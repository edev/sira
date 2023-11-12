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
///
/// To enable better integration testing, this type is public but stripped from documentation.
#[derive(Debug)]
#[doc(hidden)]
pub struct TestableClientThread {
    /// The host name that this thread is meant to manage.
    host: String,

    /// The [Sender] for reporting the progress of this specific client back to [Network] and
    /// the [Receiver] for [NetworkControlMessage]s meant specifically for this host.
    channels: ChannelPair,
}

impl TestableClientThread {
    pub fn new(
        host: String,
        sender: Sender<Report>,
        receiver: Receiver<NetworkControlMessage>,
    ) -> Self {
        let channels = ChannelPair { sender, receiver };
        TestableClientThread { host, channels }
    }

    pub fn run<S: Session>(mut self, session: &mut S) {
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
            Err(_) => return false,
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

                if !session.connected() {
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
                        manifest_source: host_action
                            .manifest()
                            .source
                            .clone()
                            .map(|s| s.to_string_lossy().to_string()),
                        manifest_name: host_action.manifest().name.to_string(),
                        task_source: host_action
                            .task()
                            .source
                            .clone()
                            .map(|s| s.to_string_lossy().to_string()),
                        task_name: host_action.task().name.to_string(),
                        action: Arc::new(host_action.action().clone()),
                    })
                    .unwrap();

                // Send the action to the host and collect the output.
                use Action::*;
                let output = match host_action.compile() {
                    action @ Shell { .. } | action @ LineInFile { .. } => session
                        // Unwrap: YAML serialization should never fail.
                        .client_action(serde_yaml::to_string(&action).unwrap())
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
                        manifest_source: host_action
                            .manifest()
                            .source
                            .clone()
                            .map(|s| s.to_string_lossy().to_string()),
                        manifest_name: host_action.manifest().name.to_string(),
                        task_source: host_action
                            .task()
                            .source
                            .clone()
                            .map(|s| s.to_string_lossy().to_string()),
                        task_name: host_action.task().name.to_string(),
                        action: Arc::new(host_action.action().clone()),
                        result: output.map_err(|e| e.to_string()),
                    })
                    .unwrap();

                // Request that the loop continue.
                true
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

                // Request that the loop terminate.
                false
            }
        }
    }
}

/// A connection to a remote host via SSH.
///
/// Each thread gets its own, newly constructed value of this type. [ClientThread] uses this value
/// to open connections, run client actions, and so on.
///
/// This lives in its own trait, as a value instantiated and passed into [TestableClientThread],
/// for dependency injection for testing.
///
/// To enable better integration testing, this type is public but stripped from documentation.
#[doc(hidden)]
pub trait Session {
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
    use std::path::PathBuf;
    use std::process::ExitStatus;
    use std::str::FromStr;

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

    #[derive(Clone, Debug, PartialEq)]
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
            use crate::core::Manifest;
            use std::ops::Deref;

            fn run_action(
                caller: &executor::ChannelPair<NetworkControlMessage, Report>,
                client: &mut TestableClientThread,
                action: &Action,
            ) -> Manifest {
                let (_, mut manifest, _, _) = plan();

                // Replace the default manifest's one action with the action the calling code
                // provided.
                manifest.include[0].actions[0] = action.clone();

                let message = Arc::new(HostAction::new(
                    &client.host,
                    &manifest,
                    &manifest.include[0],
                    &manifest.include[0].actions[0],
                ));
                caller
                    .sender
                    .send(NetworkControlMessage::RunAction(message))
                    .unwrap();

                manifest
            }

            #[test]
            #[should_panic(expected = "HostAction meant for Zork")]
            fn panics_if_wrong_host() {
                let (caller, mut client) = harness();
                let (_, mut manifest, task, action) = plan();

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

                /// Asks a client to connect. Returns the [Session] value and the return value
                /// of [TestableClientThread::_run_once].
                fn connect(
                    caller: &executor::ChannelPair<NetworkControlMessage, Report>,
                    client: &mut TestableClientThread,
                ) -> (TestSession, bool) {
                    let (_, manifest, task, action) = plan();
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
                    let (caller, mut client) = harness();

                    let (session, retval) = connect(&caller, &mut client);
                    assert!(retval);

                    assert!(session.connected());
                }

                #[test]
                fn reports_connection_success_and_continues() {
                    let (caller, mut client) = harness();

                    let (_, retval) = connect(&caller, &mut client);

                    // Verify that the loop will continue.
                    assert!(retval);

                    let reports: Vec<_> = caller.receiver.try_iter().collect();

                    // Verify that the method reported success.
                    let received_connected_message = reports
                        .iter()
                        .any(|msg| matches!(msg, Report::Connected(_)));
                    assert!(received_connected_message);

                    // Verify that the method continued on rather than returning early.
                    let received_running_action_message = reports
                        .iter()
                        .any(|msg| matches!(msg, Report::RunningAction { .. }));
                    assert!(received_running_action_message);
                }

                #[test]
                fn reports_connection_failure_and_exits() {
                    let (caller, mut client) = harness();

                    // Modified from connect(): just this test should fail to connect.
                    let (_, manifest, task, action) = plan();
                    let message =
                        Arc::new(HostAction::new(&client.host, &manifest, &task, &action));
                    caller
                        .sender
                        .send(NetworkControlMessage::RunAction(message))
                        .unwrap();

                    let mut session = TestSession::new();
                    session.connects = false;
                    let retval = client._run_once(&mut session);

                    // Verify that the loop will break.
                    assert!(!retval);

                    let reports: Vec<_> = caller.receiver.try_iter().collect();

                    // Verify that the method reported failure.
                    let received_failed_to_connect_message = reports
                        .iter()
                        .any(|msg| matches!(msg, Report::FailedToConnect { .. }));
                    assert!(received_failed_to_connect_message);

                    // Verify that the method returned early rather than continuing on.
                    let received_running_action_message = reports
                        .iter()
                        .any(|msg| matches!(msg, Report::RunningAction { .. }));
                    assert!(!received_running_action_message);
                }
            }

            #[test]
            fn reports_running_action() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the action to the client.
                let action = Action::Shell(vec!["cat cats.txt".to_string()]);
                let manifest = run_action(&caller, &mut client, &action);

                client._run_once(&mut session);

                // As of this writing, we don't implement PartialEq on Report because it uses
                // std::process::Output, which doesn't implement PartialEq. For this test, we
                // manually compare the fields of the expected report.

                // Find the report. There should be exactly 1.
                let mut received_running_action_messages: Vec<_> = caller
                    .receiver
                    .try_iter()
                    .filter(|msg| matches!(msg, Report::RunningAction { .. }))
                    .collect();
                assert_eq!(1, received_running_action_messages.len());
                let message = received_running_action_messages.pop().unwrap();

                // Assert each field.
                match message {
                    Report::RunningAction {
                        host,
                        manifest_source,
                        manifest_name,
                        task_source,
                        task_name,
                        action,
                    } => {
                        assert_eq!(client.host, host);
                        assert_eq!(
                            manifest.source,
                            manifest_source
                                .as_ref()
                                .map(|s| PathBuf::from_str(s).unwrap()),
                        );
                        assert_eq!(manifest.name, manifest_name);
                        assert_eq!(
                            manifest.include[0].source,
                            task_source.as_ref().map(|s| PathBuf::from_str(s).unwrap()),
                        );
                        assert_eq!(manifest.include[0].name, task_name);
                        assert_eq!(&manifest.include[0].actions[0], action.deref());
                    }
                    r => panic!(
                        "Bug in test! Expected Report::RunningAction but received: {:?}",
                        r
                    ),
                }
            }

            #[test]
            fn shell_calls_client_action() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the action to the client.
                let action = Action::Shell(vec!["cat cats.txt".to_string()]);
                run_action(&caller, &mut client, &action);

                client._run_once(&mut session);

                assert_eq!(
                    vec![
                        TestAction::Connect(client.host.clone()),
                        TestAction::ClientAction(serde_yaml::to_string(&action).unwrap()),
                    ],
                    session.actions,
                );
            }

            #[test]
            fn line_in_file_calls_client_action() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the action to the client.
                let action = Action::LineInFile {
                    after: "<after>".into(),
                    insert: vec!["line_1".into(), "line_2".into()],
                    path: "<path>".into(),
                };
                run_action(&caller, &mut client, &action);

                client._run_once(&mut session);

                assert_eq!(
                    vec![
                        TestAction::Connect(client.host.clone()),
                        TestAction::ClientAction(serde_yaml::to_string(&action).unwrap()),
                    ],
                    session.actions,
                );
            }

            #[test]
            fn upload_calls_scp() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                const FROM: &str = "<from>";
                const TO: &str = "<to>";

                // Send the action to the client.
                let action = Action::Upload {
                    from: FROM.to_string(),
                    to: TO.to_string(),
                };
                run_action(&caller, &mut client, &action);

                client._run_once(&mut session);

                assert_eq!(
                    vec![
                        TestAction::Connect(client.host.clone()),
                        TestAction::Scp(FROM.to_string(), format!("{}:{}", client.host, TO)),
                    ],
                    session.actions,
                );
            }

            #[test]
            fn download_calls_scp() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                const FROM: &str = "<from>";
                const TO: &str = "<to>";

                // Send the action to the client.
                let action = Action::Download {
                    from: FROM.to_string(),
                    to: TO.to_string(),
                };
                run_action(&caller, &mut client, &action);

                client._run_once(&mut session);

                assert_eq!(
                    vec![
                        TestAction::Connect(client.host.clone()),
                        TestAction::Scp(format!("{}:{}", client.host, FROM), TO.to_string()),
                    ],
                    session.actions,
                );
            }

            #[test]
            fn reports_result() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the action to the client.
                let action = Action::Download {
                    from: "<from>".into(),
                    to: "<to>".into(),
                };
                let manifest = run_action(&caller, &mut client, &action);

                client._run_once(&mut session);

                let mut results: Vec<_> = caller
                    .receiver
                    .try_iter()
                    .filter(|report| matches!(report, Report::ActionResult { .. }))
                    .collect();

                assert_eq!(1, results.len());
                match results.pop().unwrap() {
                    Report::ActionResult {
                        host,
                        manifest_source,
                        manifest_name,
                        task_source,
                        task_name,
                        action,
                        result,
                    } => {
                        assert_eq!(client.host, host);
                        assert_eq!(
                            manifest.source,
                            manifest_source
                                .as_ref()
                                .map(|s| PathBuf::from_str(s).unwrap()),
                        );
                        assert_eq!(manifest.name, manifest_name);
                        assert_eq!(
                            manifest.include[0].source,
                            task_source.as_ref().map(|s| PathBuf::from_str(s).unwrap()),
                        );
                        assert_eq!(manifest.include[0].name, task_name);
                        assert_eq!(action, action);
                        assert_eq!(
                            Ok(Output {
                                status: ExitStatus::from_raw(0),
                                stdout: vec![],
                                stderr: vec![],
                            }),
                            result,
                        );
                    }
                    other => panic!("Expected Report::ActionResult but received {:?}", other),
                }
            }

            #[test]
            fn continues() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the action to the client.
                let action = Action::Download {
                    from: "<from>".into(),
                    to: "<to>".into(),
                };
                run_action(&caller, &mut client, &action);

                let should_continue = client._run_once(&mut session);

                assert!(should_continue);
            }
        }

        mod disconnect {
            use super::*;

            #[test]
            #[should_panic(expected = "Disconnect message meant for")]
            fn panics_if_wrong_host() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Choose a wrong host.
                let host = "Bad host".to_string();
                assert_ne!(host, client.host);

                // Send the Disconnect message to the client.
                let message = NetworkControlMessage::Disconnect(host);
                caller.sender.send(message).unwrap();

                client._run_once(&mut session);
            }

            #[test]
            fn reports_disconnected() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the Disconnect message to the client.
                let message = NetworkControlMessage::Disconnect(client.host.clone());
                caller.sender.send(message).unwrap();

                client._run_once(&mut session);

                let mut reports: Vec<_> = caller
                    .receiver
                    .try_iter()
                    .filter(|msg| matches!(msg, Report::Disconnected { .. }))
                    .collect();
                assert_eq!(1, reports.len());

                assert_eq!(
                    Report::Disconnected {
                        host: client.host.clone(),
                        error: None,
                    },
                    reports.pop().unwrap(),
                );
            }

            #[test]
            fn terminates() {
                let (caller, mut client) = harness();
                let mut session = TestSession::new();

                // Send the Disconnect message to the client.
                let message = NetworkControlMessage::Disconnect(client.host.clone());
                caller.sender.send(message).unwrap();

                let should_continue = client._run_once(&mut session);

                assert!(!should_continue);
            }
        }
    }
}
