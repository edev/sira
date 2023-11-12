//! Exercises Sira from loading a [Plan] from YAML files through running simulated networked
//! [Action]s and reporting the results to a simulated [ui] and [logger].
//!
//! Key behaviors under test are the ability of the entire system to process multiple [Manifest]s
//! and multiple remote hosts correctly and without getting stuck, e.g. because of a protocol flaw.

use sira::core::Action;
use sira::core::Plan;
use sira::executor::{Executor, Report};
use sira::logger::{LogEntry, LogReceiver, Logger};
use sira::network;
use sira::reference::network::Network;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::sync::{Arc, Mutex};
use std::thread;

pub mod log {
    use super::*;

    // Copied from sira::logger::tests::log_receiver.
    //
    // We don't currently inspect the logger, because we're just checking that the system is
    // running, and checking the UI is enough for that. However, it's here if we decide we need to
    // test it in the future, and it was easy to pull in.
    #[derive(Clone, Default)]
    pub struct TestLogger {
        reports: Vec<LogEntry<Report>>,
        raw: Vec<LogEntry<String>>,
    }

    // Copied from sira::logger::tests::log_receiver.
    impl TestLogger {
        pub fn new() -> Arc<Mutex<Self>> {
            Arc::new(Mutex::new(Self {
                reports: vec![],
                raw: vec![],
            }))
        }
    }

    // We can't implement Logger for Arc<_> because of the orphan rule, so we'll use a wrapper type.
    #[derive(Clone, Default)]
    pub struct AMTestLogger(Arc<Mutex<TestLogger>>);

    impl AMTestLogger {
        pub fn new() -> Self {
            Self(TestLogger::new())
        }
    }

    impl Logger for AMTestLogger {
        fn log_raw(&mut self, entry: LogEntry<String>) {
            self.0.lock().unwrap().raw.push(entry);
        }

        fn log_report(&mut self, report: LogEntry<Report>) {
            self.0.lock().unwrap().reports.push(report);
        }
    }
}
use log::*;

pub mod ui {
    use super::*;
    use sira::ui::{ChannelPair, Message};

    pub struct TestUi {
        received: Vec<Report>,
        channels: ChannelPair,
    }

    impl TestUi {
        /// Returns a new test UI.
        pub fn new(channels: ChannelPair) -> Self {
            Self {
                received: vec![],
                channels,
            }
        }

        /// Sends `plan` to [Executor] to be run, then receives messages until the channel closes.
        ///
        /// Returns a list of received [Report]s.
        pub fn run(mut self, plan: Plan) -> Reports {
            self.channels
                .sender
                .try_send(Message::RunPlan(plan))
                .unwrap();

            while let Ok(report) = self.channels.receiver.recv() {
                let done = report == Report::Done;
                self.received.push(report);
                if done {
                    if let Ok(report) = self.channels.receiver.try_recv() {
                        panic!(
                            "Expected channel to be empty after receiving Report::Done, but \
                            received:\n\
                            {report:#?}\n\
                            \n\
                            Received reports:\n\
                            {:#?}",
                            self.received
                        );
                    }
                    break;
                }
            }
            Reports(self.received)
        }
    }

    pub struct Reports(Vec<Report>);

    impl Reports {
        /// Asserts that a particular sequence of messages arrived.
        ///
        /// Verifies that the sequence of messages arrived in the order specified, ignoring any
        /// other messages along the way. This resolves the inherent race conditions of [Plan]
        /// execution order throughout Sira, at least from the perspective of test code checking
        /// for success.
        pub fn expect(&self, messages: &[network::Report]) {
            let mut received = self.0.iter();
            for message in messages {
                assert!(
                    received.any(|r| {
                        match r {
                            Report::Done => false,
                            Report::NetworkReport(report) => report == message,
                        }
                    }),
                    "Could not find expected Report in TestUI's received messages:\n\
                    {message:#?}\n\
                    \n\
                    Full log of received messages:\n\
                    {:#?}",
                    self.0
                );
            }
        }

        /// Asserts that the messages ended with [Report::Done].
        pub fn expect_done(&self) {
            let last = self.0.last().unwrap();
            assert_eq!(
                Report::Done,
                *last,
                "Expected the last message to the UI to be Report::Done, but instead saw:\n\
                {last:#?}\n\
                \n\
                Full log of received messages:\n\
                {:#?}",
                self.0
            );
        }
    }
}
use ui::*;

// This module is named differently because we also need to refer to Sira's network module.
pub mod test_network {
    use super::*;
    use anyhow::bail;
    use crossbeam::channel::{Receiver, Sender};
    use sira::executor::NetworkControlMessage;
    use sira::reference::network::openssh::Session;
    use sira::reference::network::ClientThread;
    use std::ffi::OsStr;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    pub use sira::reference::network::openssh::TestableClientThread;

    pub struct TestClientThread(TestableClientThread);

    impl ClientThread for TestClientThread {
        fn new(
            host: String,
            sender: Sender<network::Report>,
            receiver: Receiver<NetworkControlMessage>,
        ) -> Self {
            TestClientThread(TestableClientThread::new(host, sender, receiver))
        }

        fn run(self) {
            self.0.run(&mut TestSession::new())
        }
    }

    // Copied from sira::reference::network::openssh::tests
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
}
use test_network::*;

#[test]
fn main() -> anyhow::Result<()> {
    // Create logger.
    let logger = AMTestLogger::new();
    let (log_receiver, log, executive_log) = LogReceiver::new(logger.clone());

    // Create executor, which gives us channels for UI and network.
    let (executor, ui_channels, network_channels) = Executor::new(executive_log);

    // Create network, being careful not to create a real, OpenSSH-based one.
    let network: Network<TestClientThread> = Network::new(network_channels, log);

    // Create UI.
    let ui = TestUi::new(ui_channels);

    // Start the ball rolling. If the test hangs, it's in this section.
    let ui = thread::spawn(move || -> anyhow::Result<Reports> {
        Ok(ui.run(Plan::from_manifest_files(&[
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources/test/load_manifests/manifest1.yaml"),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources/test/load_manifests/manifest2.yaml"),
        ])?))
    });
    let log_receiver = thread::spawn(|| log_receiver.run());
    let executor = thread::spawn(|| executor.run());
    let network = thread::spawn(|| network.run());

    let ui = ui.join().unwrap()?;
    log_receiver.join().unwrap();
    executor.join().unwrap()?;
    network.join().unwrap()?;

    // Verify test success.

    use sira::network::Report::*;

    // Shared values for use below.

    // Defines a function that returns an owned string.
    macro_rules! string {
        ($fn_name:ident, $str:literal) => {
            fn $fn_name() -> String {
                $str.to_string()
            }
        };
    }

    // Defines a function that returns an Option<String> pointing to a YAML file in
    // resources/test/load_manifests.
    macro_rules! load_manifests {
        ($fn_name:ident, $file_name:literal) => {
            fn $fn_name() -> Option<String> {
                Some(
                    [
                        env!("CARGO_MANIFEST_DIR"),
                        "resources/test/load_manifests",
                        $file_name,
                    ]
                    .iter()
                    .collect::<PathBuf>()
                    .to_string_lossy()
                    .to_string(),
                )
            }
        };
    }

    // Defines a function that returns an Arc<Action>.
    macro_rules! action {
        ($fn_name:ident, $action:expr) => {
            fn $fn_name() -> Arc<Action> {
                Arc::new($action)
            }
        };
    }

    string!(apt_install, "apt install");
    string!(desktops, "desktops");
    string!(set_host_name, "set host name");
    string!(snap_install, "snap install");
    string!(t470, "t470");
    string!(zen3, "zen3");

    load_manifests!(manifest1, "manifest1.yaml");
    load_manifests!(manifest2, "manifest2.yaml");
    load_manifests!(task1, "task1.yaml");
    load_manifests!(task2, "task2.yaml");
    load_manifests!(t470_yaml, "t470.yaml");
    load_manifests!(zen3_yaml, "zen3.yaml");

    action!(
        task1_apt_install,
        Action::Shell(vec!["apt install -y $packages".to_string()])
    );

    action!(
        task2_snap_install,
        Action::Shell(vec!["snap install $snaps".to_string()])
    );

    action!(
        t470_hostnamectl,
        Action::Shell(vec!["hostnamectl hostname t470".to_string()])
    );

    action!(
        zen3_hostnamectl,
        Action::Shell(vec!["hostnamectl hostname zen3".to_string()])
    );

    fn success() -> Result<Output, String> {
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: vec![],
            stderr: vec![],
        })
    }

    // Host: t470
    ui.expect(&[
        Connecting(t470()),
        Connected(t470()),
        RunningAction {
            host: t470(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task1(),
            task_name: apt_install(),
            action: task1_apt_install(),
        },
        ActionResult {
            host: t470(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task1(),
            task_name: apt_install(),
            action: task1_apt_install(),
            result: success(),
        },
        RunningAction {
            host: t470(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task2(),
            task_name: snap_install(),
            action: task2_snap_install(),
        },
        ActionResult {
            host: t470(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task2(),
            task_name: snap_install(),
            action: task2_snap_install(),
            result: success(),
        },
        RunningAction {
            host: t470(),
            manifest_source: manifest1(),
            manifest_name: t470(),
            task_source: t470_yaml(),
            task_name: set_host_name(),
            action: t470_hostnamectl(),
        },
        ActionResult {
            host: t470(),
            manifest_source: manifest1(),
            manifest_name: t470(),
            task_source: t470_yaml(),
            task_name: set_host_name(),
            action: t470_hostnamectl(),
            result: success(),
        },
        RunningAction {
            host: t470(),
            manifest_source: manifest2(),
            manifest_name: t470(),
            task_source: t470_yaml(),
            task_name: set_host_name(),
            action: t470_hostnamectl(),
        },
        ActionResult {
            host: t470(),
            manifest_source: manifest2(),
            manifest_name: t470(),
            task_source: t470_yaml(),
            task_name: set_host_name(),
            action: t470_hostnamectl(),
            result: success(),
        },
        Disconnected {
            host: t470(),
            error: None,
        },
    ]);

    // Host: zen3
    ui.expect(&[
        Connecting(zen3()),
        Connected(zen3()),
        RunningAction {
            host: zen3(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task1(),
            task_name: apt_install(),
            action: task1_apt_install(),
        },
        ActionResult {
            host: zen3(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task1(),
            task_name: apt_install(),
            action: task1_apt_install(),
            result: success(),
        },
        RunningAction {
            host: zen3(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task2(),
            task_name: snap_install(),
            action: task2_snap_install(),
        },
        ActionResult {
            host: zen3(),
            manifest_source: manifest1(),
            manifest_name: desktops(),
            task_source: task2(),
            task_name: snap_install(),
            action: task2_snap_install(),
            result: success(),
        },
        RunningAction {
            host: zen3(),
            manifest_source: manifest1(),
            manifest_name: zen3(),
            task_source: zen3_yaml(),
            task_name: set_host_name(),
            action: zen3_hostnamectl(),
        },
        ActionResult {
            host: zen3(),
            manifest_source: manifest1(),
            manifest_name: zen3(),
            task_source: zen3_yaml(),
            task_name: set_host_name(),
            action: zen3_hostnamectl(),
            result: success(),
        },
        Disconnected {
            host: zen3(),
            error: None,
        },
    ]);

    // After all of that, there should be one message left in the channel: Report::Done.
    ui.expect_done();

    Ok(())
}
