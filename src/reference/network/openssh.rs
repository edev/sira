//! Contains a basic network implementation based on the [openssh] crate.

// TODO Consider moving Connections and Client to crate::network for reuse. Make them public, if so.

use crate::core::action::Action;
use crate::executor::NetworkControlMessage;
use crate::network::{ChannelPair, Report};
use crate::reference::network::ClientThread as NetworkClientThread;
#[cfg(doc)]
use crate::reference::network::Network;
use anyhow::anyhow;
use crossbeam::channel::{Receiver, Sender};
use openssh::{KnownHosts, Session};
use std::sync::Arc;

/// Data and code for running a single client thread via OpenSSH.
struct ClientThread {
    /// The host name that this thread is meant to manage.
    host: String,

    /// The [Sender] for reporting the progress of this specific client back to [Network] and
    /// the [Receiver] for [NetworkControlMessage]s meant specifically for this host.
    channels: ChannelPair,

    /// The [Session] value representing an active SSH connection, if any.
    session: Option<Session>,
}

impl NetworkClientThread for ClientThread {
    fn new(
        host: String,
        sender: Sender<Report>,
        receiver: Receiver<NetworkControlMessage>,
    ) -> Self {
        let channels = ChannelPair { sender, receiver };
        ClientThread {
            host,
            channels,
            session: None,
        }
    }

    fn run(mut self) {
        // Tokio doesn't document when `build()` fails or why. For now, simply unwrap it; if errors
        // crop up and need addressing, we'll revisit this code.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        while let Ok(message) = self.channels.receiver.recv() {
            use NetworkControlMessage::*;
            match message {
                RunAction(host_action) => {
                    // Panic if we receive a `HostAction` meant for someone else, as this indicates
                    // a bug (probably in the routing code).
                    assert_eq!(
                        self.host,
                        host_action.host(),
                        "The thread for host {} received a HostAction meant for {}",
                        self.host,
                        host_action.host(),
                    );

                    if self.session.is_none() {
                        // Report that we're trying to connect.
                        self.channels
                            .sender
                            .send(Report::Connecting(self.host.clone()))
                            .unwrap();

                        // Block while attempting to connect.
                        let session = runtime
                            .block_on(Session::connect_mux(host_action.host(), KnownHosts::Add));

                        match session {
                            Ok(session) => {
                                // Save the session and report success.
                                self.session = Some(session);
                                self.channels
                                    .sender
                                    .send(Report::Connected(self.host.clone()))
                                    .unwrap();
                            }
                            Err(error) => {
                                // Report the failure and exit the thread.
                                self.channels
                                    .sender
                                    .send(Report::FailedToConnect {
                                        host: self.host,
                                        error: error.to_string(),
                                    })
                                    .unwrap();
                                break;
                            }
                        }
                    }

                    // By now, `self.session` is initialized.
                    let session = self.session.as_ref().unwrap();

                    // Report that we're running the action.
                    self.channels
                        .sender
                        .send(Report::RunningAction {
                            host: self.host.clone(),
                            manifest_source: host_action.manifest().source.clone(),
                            manifest_name: host_action.manifest().name.to_string(),
                            task_source: host_action.task().source.clone(),
                            task_name: host_action.task().name.to_string(),
                            action: Arc::new(host_action.action().clone()),
                        })
                        .unwrap();

                    // Send the action to the host and collect the output.
                    use Action::*;
                    let output = match host_action.action() {
                        Shell { .. } | LineInFile { .. } => runtime
                            .block_on(
                                session
                                    .command("/home/edev/.cargo/bin/sira-client")
                                    .arg("HostAction.to_yaml()")
                                    .output(),
                            )
                            .map_err(|e| anyhow!(e)),

                        // There's a lot missing from this implementation:
                        //
                        // - User, group, and permissions
                        // - Privilege escalation for transferring files using, e.g., sudo/su.
                        //
                        // For now, this runs as the SSH user and uses default permissions. If you
                        // want to do something more complex, you'll have to use Action::Shell
                        // before/after Action::Upload. However, this really needs a
                        // well-considered solution!
                        //
                        // TODO Solve permissions & other issues with Upload & Download.
                        //
                        // The solution to a lot of this is probably honestly to upload to a
                        // temporary location owned by the SSH user and then run a hidden action to
                        // chown, chmod, and mv under root privileges. The main reason I haven't
                        // done so already is that I'm not yet prepared to commit to either a
                        // single privilege escalation strategy or a system for managing different
                        // strategies.

                        // Run the local `scp` program that comes with OpenSSH.
                        Upload { from, to } => std::process::Command::new("scp")
                            .arg(from)
                            .arg(format!("{}:{}", self.host.clone(), to))
                            .output()
                            .map_err(|e| anyhow!(e)),

                        // See notes for Upload in this same match statement.
                        Download { from, to } => std::process::Command::new("scp")
                            .arg(format!("{}:{}", self.host.clone(), from))
                            .arg(to)
                            .output()
                            .map_err(|e| anyhow!(e)),
                    };

                    // Report the action's outcome.
                    self.channels
                        .sender
                        .send(Report::ActionResult {
                            host: self.host.clone(),
                            manifest_source: host_action.manifest().source.clone(),
                            manifest_name: host_action.manifest().name.to_string(),
                            task_source: host_action.task().source.clone(),
                            task_name: host_action.task().name.to_string(),
                            action: Arc::new(host_action.action().clone()),
                            result: output,
                        })
                        .unwrap();
                }
                Disconnect(host) => {
                    // Panic if we receive a `HostAction` meant for someone else, as this indicates
                    // a bug (probably in the routing code).
                    assert_eq!(
                        self.host, host,
                        "The thread for host {} received a Disconnect message meant for {}",
                        self.host, host,
                    );

                    self.channels
                        .sender
                        .send(Report::Disconnected {
                            host: self.host.clone(),
                            error: None,
                        })
                        .unwrap();
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod new {
        use super::*;

        #[test]
        fn works() {
            use crate::executor;
            use crossbeam::channel;

            // Set up channels for communicating with a ClientThread.
            let (report_send, report_recv) = channel::unbounded();
            let (control_send, control_recv) = channel::unbounded();

            // The caller's channels.
            let caller: executor::ChannelPair<NetworkControlMessage, Report> =
                executor::ChannelPair {
                    sender: control_send,
                    receiver: report_recv,
                };

            // The ClientThread's channels, stored here momentarily for clarity.
            let client_thread_channels: executor::ChannelPair<Report, NetworkControlMessage> =
                executor::ChannelPair {
                    sender: report_send,
                    receiver: control_recv,
                };

            let client_thread = ClientThread::new(
                "archie".into(),
                client_thread_channels.sender,
                client_thread_channels.receiver,
            );

            assert_eq!("archie", client_thread.host);

            // Verify that the channels are set up correctly. Since this is single-threaded code,
            // there shouldn't be any race conditions over sending and receiving.

            caller
                .sender
                .send(NetworkControlMessage::Disconnect("archie".to_string()))
                .unwrap();
            assert_eq!(
                Ok(NetworkControlMessage::Disconnect("archie".to_string())),
                client_thread.channels.receiver.try_recv(),
            );

            client_thread
                .channels
                .sender
                .send(Report::Connecting("archie".into()))
                .unwrap();
            let msg = caller.receiver.try_recv();

            // We can't simply use assert_eq! here, because Report can't implement PartialEq.
            if let Ok(Report::Connecting(host)) = msg {
                assert_eq!("archie", host);
            } else {
                panic!(
                    "Expected Report::Connecting(\"archie\") but received {:?}",
                    msg,
                );
            }
        }
    }

    mod run {
        use super::*;
    }
}
