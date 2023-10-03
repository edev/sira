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
use std::convert::AsRef;
use std::ffi::OsStr;
use std::process::Output;
use std::sync::Arc;

/// Data and code for running a single client thread via OpenSSH.
#[derive(Debug)]
pub struct ClientThread {
    wrapped: TestableClientThread,
}

impl NetworkClientThread for ClientThread {
    fn new(
        host: String,
        sender: Sender<Report>,
        receiver: Receiver<NetworkControlMessage>,
    ) -> Self {
        ClientThread {
            wrapped: TestableClientThread::new(host, sender, receiver),
        }
    }

    fn run(self) {
        self.wrapped.run(&mut OpenSSHSession::new())
    }
}

/// A private, testable implementation of [ClientThread] that adds dependency injection.
///
/// # Design notes
///
/// In order to properly test the OpenSSH implementation of [ClientThread], we need to be able to
/// fake all calls to OpenSSH. This means either abstracting or duplicating the [openssh] API. The
/// approach we choose is to abstract it into [Session]. [TestableClientThread] has almost the same
/// API as [NetworkClientThread], except that it adds the injection of a value that implements
/// [Session] to [Self::run].
///
/// We could have pushed this injection farther up, e.g. to [NetworkClientThread], but this would
/// have required us to define the connection API for all possible connection types. Instead, we
/// have opted to keep that totally unrestricted outside of [crate::reference::network::openssh].
/// Thus, we need [TestableClientThread] to pass in a [Session] value so that we can use a fake one
/// for testing.
///
/// # Testing
///
/// Some tests do not call the methods we fake with [Session]. For thoroughness, these tests use
/// [ClientThread] instead of [TestableClientThread].
#[derive(Debug)]
struct TestableClientThread {
    /// The host name that this thread is meant to manage.
    host: String,

    /// The [Sender] for reporting the progress of this specific client back to [Network] and
    /// the [Receiver] for [NetworkControlMessage]s meant specifically for this host.
    channels: ChannelPair,
}

impl TestableClientThread {
    fn new(
        host: String,
        sender: Sender<Report>,
        receiver: Receiver<NetworkControlMessage>,
    ) -> Self {
        let channels = ChannelPair { sender, receiver };
        TestableClientThread { host, channels }
    }

    fn run<S: Session>(mut self, session: &mut S) {
        while self._run_once(session) {}
    }

    /// The inner body of the [Self::run] loop.
    ///
    /// Breaking it out this way allows for much better testing: it's easier to step through, and
    /// it's guaranteed to terminate.
    ///
    /// Returns whether to continue looping. Returning `false` is equivalent to requesting a
    /// `break` in the [Self::run] loop.
    fn _run_once<S: Session>(&mut self, session: &mut S) -> bool {
        // Block until we receive the next message, or return if the channel is empty and closed.
        let message = match self.channels.receiver.recv() {
            Ok(message) => message,
            Err(e) => return false,
        };

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

                if session.connected() {
                    // Report that we're trying to connect.
                    self.channels
                        .sender
                        .send(Report::Connecting(self.host.clone()))
                        .unwrap();

                    // Block while attempting to connect.
                    match session.connect(host_action.host()) {
                        Ok(()) => {
                            // Report success and continue.
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
                                    host: self.host.clone(),
                                    error: error.to_string(),
                                })
                                .unwrap();
                            return false;
                        }
                    }
                }

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
                let output = match host_action.compile() {
                    action @ Shell { .. } | action @ LineInFile { .. } => session
                        .client_action("action.to_yaml()")
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
                    Upload { from, to } => {
                        let to = format!("{}:{}", self.host.clone(), to);
                        session.scp(from, to)
                    }

                    // See notes for Upload in this same match statement.
                    Download { from, to } => {
                        let from = format!("{}:{}", self.host.clone(), from);
                        session.scp(from, to)
                    }
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
                return false;
            }
        }
        true
    }
}

/// A connection to a remote host via SSH.
///
/// Each thread gets its own, newly constructed value of this type. [ClientThread] uses this value
/// to open connections, run client actions, and so on.
///
/// This lives in its own trait, as a value instantiated and passed into [TestableClientThread],
/// for dependency injection for testing.
trait Session {
    /// Returns whether [Self::connect] has ever both been called and succeeded (returned [Ok]).
    ///
    /// For efficiency, this method does not check whether the network is actually connected, as
    /// that takes more work from both [tokio] and [openssh].
    fn connected(&self) -> bool;

    /// Attempts to open a connection to a remote host.
    ///
    /// The exact format of `destination` is left as an implementation detail to be negotiated
    /// between [Session] and [ClientThread] implementations. However, a sane default is the
    /// network name of the remote host.
    ///
    /// Note that there is no disconnect method, because each thread must to exit after its work is
    /// done rather than closing and reopening connections. Implementers must ensure that the
    /// connection closes when the [Session] value is dropped.
    fn connect(&mut self, destination: impl AsRef<str>) -> anyhow::Result<()>;

    /// Invokes `sira-client` on the remote host, passing `arg` as the first and only argument.
    ///
    /// Typically, `arg` will be a serialized [Action].
    fn client_action(&mut self, arg: impl AsRef<str>) -> anyhow::Result<Output>;

    /// Invokes `scp` on the controller.
    fn scp(&mut self, from: impl AsRef<OsStr>, to: impl AsRef<OsStr>) -> anyhow::Result<Output>;
}

/// An implementation of [Session] using the [openssh] crate. For production use.
struct OpenSSHSession {
    /// The Tokio runtime. We need this so we can run async tasks using [block_on].
    ///
    /// [block_on]: tokio::runtime::Runtime::block_on
    runtime: tokio::runtime::Runtime,

    /// The active session we're using to send client actions.
    session: Option<openssh::Session>,
}

impl OpenSSHSession {
    /// Constructs a new value of this type. Does not open a connection.
    ///
    /// [ClientThread] will construct a new value of this type in each client thread and then pass
    /// it to [TestableClientThread::run].
    fn new() -> Self {
        // Tokio doesn't document when `build()` fails or why. For now, simply unwrap it; if errors
        // crop up and need addressing, we'll revisit this code.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        OpenSSHSession {
            runtime,
            session: None,
        }
    }
}

impl Session for OpenSSHSession {
    fn connected(&self) -> bool {
        self.session.is_some()
    }

    fn connect(&mut self, destination: impl AsRef<str>) -> anyhow::Result<()> {
        // It's debatable whether we should return success or panic here, as the caller really
        // isn't supposed to call this method on an already-open connection. Since it indicates a
        // bug in the calling code, we will panic.
        if self.session.is_some() {
            panic!("Tried to connect to a client using an already-connected Session");
        }

        let session = self
            .runtime
            .block_on(openssh::Session::connect_mux(destination, KnownHosts::Add));

        self.session = match session {
            Ok(session) => Some(session),
            Err(error) => return Err(anyhow!(error)),
        };

        Ok(())
    }

    fn client_action(&mut self, arg: impl AsRef<str>) -> anyhow::Result<Output> {
        let session = match self.session {
            Some(ref session) => session,
            None => panic!("You must call connect() before client_action()"),
        };

        self.runtime
            .block_on(
                session
                    .command("/home/edev/.cargo/bin/sira-client")
                    .arg(arg)
                    .output(),
            )
            .map_err(|e| anyhow!(e))
    }

    fn scp(&mut self, from: impl AsRef<OsStr>, to: impl AsRef<OsStr>) -> anyhow::Result<Output> {
        std::process::Command::new("scp")
            .arg(from)
            .arg(to)
            .output()
            .map_err(|e| anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixtures::plan;
    use crate::executor;
    use anyhow::bail;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    /// Newtype for [KnownHosts] that supports PartialEq.
    #[derive(Clone, Debug, PartialEq)]
    enum TestKnownHosts {
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

    enum TestAction {
        Connect(String),
        ClientAction(String),
        Scp(String, String),
    }

    /// A fake [Session] that logs actions taken.
    struct TestSession {
        /// Whether [Session::connect] should pretend to succeed. Defaults to true.
        connects: bool,

        /// Whether actions should return success. Defaults to true.
        ///
        /// This applies to [Session::client_action] and [Session::scp]. It does not apply to
        /// [Session::connect]; see [Self::connects].
        actions_succeed: bool,

        /// A log of all actions taken, in order.
        actions: Vec<TestAction>,
    }

    impl TestSession {
        fn new() -> Self {
            Self {
                connects: true,
                actions_succeed: true,
                actions: vec![],
            }
        }

        /// Records a [TestAction] and returns either success or failure based on
        /// [Self::actions_succeed].
        fn record_action(&mut self, action: TestAction) -> anyhow::Result<Output> {
            self.actions.push(action);

            if self.actions_succeed {
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: vec![],
                    stderr: vec![],
                })
            } else {
                bail!("Could not connect");
            }
        }
    }

    impl Session for TestSession {
        fn connected(&self) -> bool {
            self.actions
                .iter()
                .any(|a| matches!(a, TestAction::Connect(_)))
        }

        fn connect(&mut self, destination: impl AsRef<str>) -> anyhow::Result<()> {
            self.actions
                .push(TestAction::Connect(destination.as_ref().to_string()));

            if self.connects {
                Ok(())
            } else {
                bail!("Could not connect");
            }
        }

        fn client_action(&mut self, arg: impl AsRef<str>) -> anyhow::Result<Output> {
            self.record_action(TestAction::ClientAction(arg.as_ref().to_string()))
        }

        fn scp(
            &mut self,
            from: impl AsRef<OsStr>,
            to: impl AsRef<OsStr>,
        ) -> anyhow::Result<Output> {
            self.record_action(TestAction::Scp(
                from.as_ref().to_string_lossy().into_owned(),
                to.as_ref().to_string_lossy().into_owned(),
            ))
        }
    }

    fn harness() -> (
        executor::ChannelPair<NetworkControlMessage, Report>,
        TestableClientThread,
    ) {
        use crossbeam::channel;

        // Set up channels for communicating with a ClientThread.
        let (report_send, report_recv) = channel::unbounded();
        let (control_send, control_recv) = channel::unbounded();

        // The caller's channels.
        let caller: executor::ChannelPair<NetworkControlMessage, Report> = executor::ChannelPair {
            sender: control_send,
            receiver: report_recv,
        };

        // The ClientThread's channels, stored here momentarily for clarity.
        let client_thread_channels: executor::ChannelPair<Report, NetworkControlMessage> =
            executor::ChannelPair {
                sender: report_send,
                receiver: control_recv,
            };

        // Incredibly wasteful, but just for correctness, maintainability, etc., pull the host host
        // from the [plan] fixture's manifest.
        let (_, mut manifest, _, _) = plan();
        let host = manifest.hosts.pop().unwrap();

        let client_thread = TestableClientThread::new(
            host,
            client_thread_channels.sender,
            client_thread_channels.receiver,
        );

        (caller, client_thread)
    }

    mod new {
        use super::*;

        #[test]
        fn works() {
            let (caller, client_thread) = harness();

            // Since we can, we'll explicitly use the public type.
            let client_thread = ClientThread::new(
                client_thread.host,
                client_thread.channels.sender,
                client_thread.channels.receiver,
            );

            assert_eq!("archie-desktop", client_thread.wrapped.host);

            // Verify that the channels are set up correctly. Since this is single-threaded code,
            // there shouldn't be any race conditions over sending and receiving.

            caller
                .sender
                .send(NetworkControlMessage::Disconnect(
                    "archie-desktop".to_string(),
                ))
                .unwrap();
            assert_eq!(
                Ok(NetworkControlMessage::Disconnect(
                    "archie-desktop".to_string(),
                )),
                client_thread.wrapped.channels.receiver.try_recv(),
            );

            client_thread
                .wrapped
                .channels
                .sender
                .send(Report::Connecting("archie-desktop".into()))
                .unwrap();
            let msg = caller.receiver.try_recv();

            // We can't simply use assert_eq! here, because Report can't implement PartialEq.
            if let Ok(Report::Connecting(host)) = msg {
                assert_eq!("archie-desktop", host);
            } else {
                panic!(
                    "Expected Report::Connecting(\"archie-desktop\") but received {:?}",
                    msg,
                );
            }
        }
    }

    mod run {
        use super::*;
        use crate::core::action::HostAction;

        mod run_action {
            use super::*;

            #[test]
            #[should_panic(expected = "HostAction meant for Zork")]
            fn panics_if_wrong_host() {
                let (caller, mut client) = harness();
                let (plan, mut manifest, task, action) = plan();

                const WRONG_HOST: &str = "Zork";
                assert_ne!(WRONG_HOST, client.host);

                // Add WRONG_HOST to the manifest. Otherwise, `HostAction::new` will panic, and
                // rightly so.
                manifest.hosts.push(WRONG_HOST.to_string());

                let message = Arc::new(HostAction::new(WRONG_HOST, &manifest, &task, &action));
                caller
                    .sender
                    .send(NetworkControlMessage::RunAction(message))
                    .unwrap();

                let mut session = TestSession::new();
                client._run_once(&mut session);
            }

            mod when_not_connected {
                use super::*;

                /// Asks a client to connect. Passes through the return value of
                /// [TestableClientThread::_run_once].
                fn connect(
                    caller: &executor::ChannelPair<NetworkControlMessage, Report>,
                    client: &mut TestableClientThread,
                ) -> (TestSession, bool) {
                    let (plan, mut manifest, task, action) = plan();
                    let message =
                        Arc::new(HostAction::new(&client.host, &manifest, &task, &action));
                    caller
                        .sender
                        .send(NetworkControlMessage::RunAction(message))
                        .unwrap();

                    let mut session = TestSession::new();
                    let retval = client._run_once(&mut session);
                    (session, retval)
                }

                #[test]
                fn reports_connecting() {
                    let (caller, mut client) = harness();

                    let (_, retval) = connect(&caller, &mut client);
                    assert!(retval);

                    assert!(matches!(
                        caller.receiver.try_recv().unwrap(),
                        Report::Connecting(_),
                    ));
                }

                #[test]
                fn connects() {
                    todo!()
                }

                #[test]
                fn reports_success() {
                    todo!()
                }

                #[test]
                fn reports_failure_and_exits() {
                    todo!()
                }
            }
        }

        mod disconnect {
            use super::*;

            // terminates
            // panics_if_wrong_host
            // reports_disconnected
        }
    }
}
