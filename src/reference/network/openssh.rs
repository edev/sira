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
use openssh::{self, KnownHosts};
use std::process::Output;
use std::sync::Arc;

/// Data and code for running a single client thread via OpenSSH.
#[derive(Debug)]
pub struct ClientThread<S: Session = OpenSSHSession> {
    /// The host name that this thread is meant to manage.
    host: String,

    /// The [Sender] for reporting the progress of this specific client back to [Network] and
    /// the [Receiver] for [NetworkControlMessage]s meant specifically for this host.
    channels: ChannelPair,

    /// The [Session] value representing an active SSH connection, if any.
    session: Option<S>,
}

impl<S: Session> NetworkClientThread for ClientThread<S> {
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
                        let session = S::connect(host_action.host(), KnownHosts::Add);

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
                        Shell { .. } | LineInFile { .. } => session
                            .client_action("HostAction.to_yaml()")
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

/// An interface to a real or fake network client.
///
/// Used for dependency injection during testing. For production, use [OpenSSHSession].
pub trait Session: Sized {
    /// Opens a session to `destination`.
    fn connect<D: AsRef<str>>(destination: D, check: KnownHosts) -> anyhow::Result<Self>;

    /// Runs `sira-client` on the remote host, passing `action` as the first and only argument.
    fn client_action<A: AsRef<str>>(&self, action: A) -> anyhow::Result<Output>;
}

/// An implementation of [Session] using the [openssh] crate. For production use.
pub struct OpenSSHSession {
    /// The Tokio runtime. We need this so we can run async tasks using [block_on].
    ///
    /// [block_on]: tokio::runtime::Runtime::block_on
    runtime: tokio::runtime::Runtime,

    /// The active session we're using to send client actions.
    session: openssh::Session,
}

impl Session for OpenSSHSession {
    fn connect<D: AsRef<str>>(destination: D, check: KnownHosts) -> anyhow::Result<Self> {
        // Tokio doesn't document when `build()` fails or why. For now, simply unwrap it; if errors
        // crop up and need addressing, we'll revisit this code.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let session = runtime.block_on(openssh::Session::connect_mux(destination, check));

        match session {
            Ok(session) => Ok(OpenSSHSession { runtime, session }),
            Err(error) => Err(anyhow!(error)),
        }
    }

    fn client_action<A: AsRef<str>>(&self, action: A) -> anyhow::Result<Output> {
        self.runtime
            .block_on(
                self.session
                    .command("/home/edev/.cargo/bin/sira-client")
                    .arg("HostAction.to_yaml()")
                    .output(),
            )
            .map_err(|e| anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::ops::Deref;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::sync::Mutex;

    /// Newtype for [KnownHosts] that supports PartialEq.
    #[derive(Clone, Debug, PartialEq)]
    pub enum TestKnownHosts {
        Strict,
        Add,
        Accept,
    }

    impl From<KnownHosts> for TestKnownHosts {
        fn from(value: KnownHosts) -> Self {
            match value {
                KnownHosts::Strict => TestKnownHosts::Strict,
                KnownHosts::Add => TestKnownHosts::Add,
                KnownHosts::Accept => TestKnownHosts::Accept,
            }
        }
    }

    /// A fake [Session] that returns failure when connecting.
    ///
    /// The implementation of [Session] for this type never instantiates this type, since its job is
    /// to fail. However, if you wish to track [Session::connect] calls, you may do the following:
    ///
    /// 1. Define a static global for each individual test, e.g. a `Mutex<Vec<TestSessionFailure>>`.
    ///
    /// 2. Write a newtype for [TestSessionFailure] that implements [Session]. This wrapper should
    ///    add [TestSessionFailure] values to your static global and then call
    ///    [TestSessionFailure]'s implementations of [Session].
    ///
    /// 3. After running the code under test, inspect your global variable.
    #[derive(Clone, Debug, PartialEq)]
    pub struct TestSessionFailure {
        /// The `destination` value passed to [Self::connect].
        destination: String,

        /// The `check` value passed to [Self::connect].
        check: TestKnownHosts,
    }

    impl Session for TestSessionFailure {
        /// Fails immediately.
        fn connect<D: AsRef<str>>(destination: D, check: KnownHosts) -> anyhow::Result<Self> {
            bail!("Could not connect");
        }

        /// Panics, as it should be impossible to call this from the code under test.
        fn client_action<A: AsRef<str>>(&self, action: A) -> anyhow::Result<Output> {
            panic!(
                "You should not be calling client_action on TestSessionFailure. \
                There should be no self."
            );
        }
    }

    /// A fake [Session] that returns success when connecting.
    ///
    /// If you wish to track [Session::connect] calls, you may do the following:
    ///
    /// 1. Define a static global for each individual test that will track [Session::connect] calls,
    ///    e.g. a `Mutex<Vec<(String, TestKnownHosts)>`.
    ///
    /// 2. Define a newtype for [TestSessionSuccess] that implements [Session] and [Drop]:
    ///
    ///     * [Session::connect] should record its arguments to the static global and then call
    ///     the underlying implementation.
    ///
    ///     * [Session::client_action] should simply wrap the underlying implementation.
    ///
    ///     * [Drop::drop] should assert that [TestSessionSuccess::actions] is set correctly.
    ///
    /// 3. After running the code under test, inspect your global variable.
    #[derive(Debug)]
    pub struct TestSessionSuccess {
        /// The `destination` value passed to [Self::connect].
        destination: String,

        /// The `check` value passed to [Self::connect].
        check: TestKnownHosts,

        /// Records every action passed to a [Self::client_action] call, in order.
        ///
        /// Mutex is used for thread-safe interior mutability.
        actions: Mutex<Vec<String>>,
    }

    impl PartialEq for TestSessionSuccess {
        fn eq(&self, other: &Self) -> bool {
            self.destination == other.destination
                && self.check == other.check
                && self.actions.lock().unwrap().deref() == other.actions.lock().unwrap().deref()
        }
    }

    impl Session for Arc<TestSessionSuccess> {
        /// Always suceeds.
        fn connect<D: AsRef<str>>(destination: D, check: KnownHosts) -> anyhow::Result<Self> {
            Ok(Arc::new(TestSessionSuccess {
                destination: destination.as_ref().to_string(),
                check: check.into(),
                actions: Mutex::new(vec![]),
            }))
        }

        /// Records the action and pretends that it succeeded, with a blank [Output] value.
        fn client_action<A: AsRef<str>>(&self, action: A) -> anyhow::Result<Output> {
            self.actions
                .lock()
                .unwrap()
                .push(action.as_ref().to_string());
            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            })
        }
    }

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

            let client_thread: ClientThread<OpenSSHSession> = ClientThread::new(
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

    /*
    mod run {
        use super::*;

        mod run_action {
            use super::*;

            panics_if_wrong_host
            mod with_no_session {
                reports_connecting

            }
        }

        mod disconnect {
            use super::*;

            terminates
            panics_if_wrong_host
            reports_disconnected
        }
    }
    */
}
