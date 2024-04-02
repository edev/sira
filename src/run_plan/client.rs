//! Provides an interface to execute [Action]s over SSH.
//!
//! [Action]: crate::core::Action

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
        to: &str,
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

    async fn upload(
        &mut self,
        from: &str,
        to: &str,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> anyhow::Result<Output> {
        let to = format!("{}:{}", self.host, to);
        let _ = self.scp(from, &to).await?;
        Ok(self.client_command(yaml, signature).await?)
    }

    async fn download(&mut self, from: &str, to: &str) -> io::Result<Output> {
        let from = format!("{}:{}", self.host, from);
        self.scp(&from, to).await
    }
}

impl Client {
    /// Invoke `sira-client` on the remote host and pass it `yaml`.
    async fn client_command(
        &mut self,
        yaml: &str,
        signature: Option<Vec<u8>>,
    ) -> Result<Output, openssh::Error> {
        let mut command = self.session.command("sira-client");
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
