//! Provides an interface to execute [Action]s over SSH.
//!
//! [Action]: crate::core::Action

use crate::core::action::FILE_TRANSFER_PATH;
use async_trait::async_trait;
use openssh::{KnownHosts, Session};
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Output};
use tokio::task;

/// Connects to clients and returns values representing those connections.
#[async_trait]
pub trait ManageClient<CI: ClientInterface> {
    /// Connect to `host` and, on success, return an interface to the host.
    async fn connect(&mut self, host: &str) -> anyhow::Result<CI>;
}

/// The interface that Sira uses to talk to clients. Maps directly to [Action].
///
/// [Action]: crate::core::Action
#[async_trait]
pub trait ClientInterface {
    /// Sends one or more commands to be run on the client.
    async fn command(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error>;

    /// Modifies a file on the client.
    async fn line_in_file(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error>;

    /// Runs a script on the client.
    async fn script(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error>;

    /// Uploads a file from the Sira control node to the client over SSH.
    async fn upload(
        &mut self,
        from: &str,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> anyhow::Result<Output>;
}

/// Production implementation of [ManageClient].
#[derive(Clone)]
pub struct ConnectionManager;

#[async_trait]
impl ManageClient<Client> for ConnectionManager {
    async fn connect(&mut self, host: &str) -> anyhow::Result<Client> {
        Ok(Client {
            session: openssh::Session::connect_mux(host, KnownHosts::Add).await?,
            host: host.to_owned(),
        })
    }
}

/// Production implementation of [ClientInterface].
pub struct Client {
    session: Session,
    host: String,
}

#[async_trait]
impl ClientInterface for Client {
    async fn command(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error> {
        self.client_command(yaml, signature).await
    }

    async fn line_in_file(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error> {
        self.client_command(yaml, signature).await
    }

    async fn script(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error> {
        self.client_command(yaml, signature).await
    }

    async fn upload(
        &mut self,
        from: &str,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> anyhow::Result<Output> {
        let to = format!("{}:{}", self.host, FILE_TRANSFER_PATH);

        // TL;DR It's very important to `rm -rf` the FILE_TRANSFER_PATH right before we call `scp`
        // to upload a file.
        //
        // There shouldn't be anything at FILE_TRANSFER_PATH on the managed node. However, if there
        // is, it might actually be a directory. In that case, scp will auto-transfer the file into
        // that directory rather than overwriting or failing. From there, subsequent logic will
        // misbehave badly. Thus, it's important to forcibly remove anything at the destination
        // just before we run scp. See below for further discussion of specific security concerns.
        //
        // `rm -rf` DOES NOT traverse symbolic links: if the path is a symbolic link, `rm` removes
        // it. Thus, doing this actually helps protect against symbolic link squatting.
        //
        // There are TOCTOU issues with all of this, but they are unavoidable. Calling `rm -rf`
        // should never make things worse due to a TOCTOU issue, as far as I know.
        //
        // Hard links are not a concern, either, since most systems will not allow hard linking to a
        // directory. An attacker managing to create a hard link here will simply find the hard
        // link removed harmlessly.
        //
        // Furthermore, an attacker who changes permissions on the file or directory still won't
        // thwart our clean-up, since the Sira user should own its own home directory. If this is
        // compromised, then the attacker likely has root access already; alternatively, if the
        // administrator makes any of several changes that result in the Sira user not owning the
        // directory in which SSH sessions start, then they have created all sorts of problems that
        // are out of our control.
        let _ = self
            .session
            .command("rm")
            .arg("-rf")
            .arg(FILE_TRANSFER_PATH)
            .status()
            .await;

        let scp_output = self.scp(from, &to).await?;
        if !scp_output.status.success() {
            return Ok(scp_output);
        }
        Ok(self.client_command(yaml, signature).await?)
    }
}

impl Client {
    /// Transfers an action and optionally its signature to the remote host and runs `sira-client`.
    async fn client_command(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error> {
        // First, write temporary files on the remote host.
        //
        // Note that we don't need to be nearly as careful when handling mktemp here as in
        // crate::client::mktemp(), because we are writing signed data.
        impl Client {
            // Writes `contents` to a temporary file on the remote host. On success, returns
            // the path to the remote temporary file.
            async fn transfer_file(
                &self,
                contents: &[u8],
            ) -> Result<String, Result<Output, openssh::Error>> {
                // Unwrap: we can't proceed without mktemp(), and it tries to output clear errors.
                let (mut local_file, local_path) = crate::client::mktemp().unwrap();
                let remote_path = match self.remote_mktemp().await {
                    Ok(s) => s,
                    Err(result) => return Err(result),
                };

                // Write a local temp file so we can call scp to transfer it.
                local_file
                    .write_all(contents)
                    .expect("error writing temp file");
                local_file.flush().expect("error flushing temp file");

                // Transfer the file via scp.
                let scp_output = self
                    .scp(&local_path, &format!("{}:{}", self.host, remote_path))
                    .await
                    .expect("error transferring temp file");
                if !scp_output.status.success() {
                    return Err(Ok(scp_output));
                }

                // Remove the local temp file.
                fs::remove_file(&local_path).expect("error removing temp file on control node");

                Ok(remote_path)
            }

            // Calls `mktemp` on the remote host, handling errors that might arise. On success,
            // returns the path to the remote file, which will be empty and ready for writing.
            async fn remote_mktemp(&self) -> Result<String, Result<Output, openssh::Error>> {
                match self.session.command("mktemp").output().await {
                    Ok(x) if !x.status.success() => Err(Ok(x)),
                    Ok(x) if !x.stderr.is_empty() => Err(Ok(x)),
                    Err(e) => Err(Err(e)),
                    Ok(x) if x.stdout.is_empty() => {
                        panic!("bug: remote mktemp exited successfully without printing anything");
                    }
                    Ok(x) => Ok(String::from_utf8_lossy(&x.stdout).trim_end().to_owned()),
                }
            }
        }

        let remote_action_path = match self.transfer_file(yaml.as_bytes()).await {
            Ok(s) => s,
            Err(result) => return result,
        };
        let remote_signature_path = match signature {
            Some(ref signature) => match self.transfer_file(signature).await {
                Ok(s) => Some(s),
                Err(result) => return result,
            },
            None => None,
        };

        let mut command = self.session.command("sudo");
        command.arg("/opt/sira/bin/sira-client");
        command.arg(remote_action_path);
        if let Some(s) = remote_signature_path {
            command.arg(s);
        }
        command.output().await
    }

    /// Invokes `scp` on the Sira control node.
    ///
    /// `from` and `to` need to be formatted correctly for use in an `scp` invocation. The command
    /// `scp <from> <to>` will be invoked directly, with no further modifications.
    async fn scp(&self, from: &str, to: &str) -> io::Result<Output> {
        task::block_in_place(move || Command::new("scp").arg(from).arg(to).output())
    }
}
