//! Provides an interface to execute [Action]s over SSH.
//!
//! [Action]: crate::core::Action

use crate::core::action::FILE_TRANSFER_PATH;
use async_trait::async_trait;
use openssh::{KnownHosts, Session};
use std::io;
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
    /// Send one or more shell commands to be run on the client.
    async fn shell(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error>;

    /// Modify a file on the client.
    async fn line_in_file(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error>;

    /// Upload a file from the Sira control node to the client over SSH.
    async fn upload(
        &mut self,
        from: &str,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> anyhow::Result<Output>;

    /// Download a file from the client to the Sira control node over SSH.
    async fn download(&mut self, from: &str, to: &str) -> io::Result<Output>;
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
    async fn shell(
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

    // TODO Strongly considering adding automatic recursive upload/download. Remember to modify
    // sira-client accordingly!
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

    async fn download(&mut self, from: &str, to: &str) -> io::Result<Output> {
        let from = format!("{}:{}", self.host, from);
        self.scp(&from, to).await
    }
}

impl Client {
    /// Invoke `sudo /opt/sira/bin/sira-client <yaml> <signature>` on the remote host.
    async fn client_command(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error> {
        let mut command = self.session.command("sudo");
        command.arg("/opt/sira/bin/sira-client");
        command.arg(yaml);
        if let Some(sig) = signature {
            let sig = String::from_utf8(sig)
                .expect("expected signature to be Base64-encoded, but it was not valid UTF-8");
            command.arg(&sig);
        }
        command.output().await
    }

    /// Invoke `scp` on the Sira control node.
    ///
    /// `from` and `to` need to be formatted correctly for use in an `scp` invocation. The command
    /// `scp <from> <to>` will be invoked directly, with no further modifications.
    async fn scp(&self, from: &str, to: &str) -> io::Result<Output> {
        task::block_in_place(move || Command::new("scp").arg(from).arg(to).output())
    }
}
