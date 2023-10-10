//! The central component of Sira's controller-side software.
//!
//! Provides the communication hub among the user interface, logger, network, and any plans
//! being run. Coordinates the execution of plans on managed nodes.
//!
//! Sira uses an hourglass design: everything communicates through a simple, well-defined interface
//! routed through [Executor], and each component is free to implement any logic or structure
//! within its own scope. The logger may arrange itself in any fashion; the user interface may be
//! simple or complex, textual or graphical, etc.; the "network connection" may be SSH
//! (the default), something else, or even some arbitrary non-networked method of connecting to
//! managed nodes.
//!
//! # Modularity
//!
//! The components that connect to the [Executor] are designed to be swapped out freely if you
//! desire. The [Executor] itself, however, is not replaceable. It provides the core logic that
//! drives program flow and acts as the glue that binds all the modular components together;
//! writing your own [Executor] would essentially be rewriting Sira itself. Thus, this module does
//! not promise to export the types necessary to write a custom [Executor].

#[cfg(doc)]
use crate::core::action::Action;
use crate::core::action::HostAction;
use crate::core::plan::{HostPlanIntoIter, Plan};
#[cfg(doc)]
use crate::logger;
use crate::logger::ExecutiveLog;
use crate::network;
use crate::ui;
use crossbeam::channel::{self, Receiver, Sender, TryRecvError};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Coordinates message routing, plan execution, and program flow.
#[allow(dead_code)]
pub struct Executor {
    /// Channels for communicating to the UI.
    ///
    /// The [ui] tells [Executor] what [Plan]s to run, and [Executor] reports progress so the
    /// [ui] can inform the user.
    ui: ChannelPair<Report, ui::Message>,

    /// A special logging interface, unavailable to any party except [Executor]. Any
    /// [network::Report]s that [Executor] receives are duplicated and sent here to be logged.
    logger: ExecutiveLog,

    /// Channels for communicating with the [network].
    ///
    /// [Executor] tells the [network] to run actions on nodes, and the network [network::Report]s
    /// its progress.
    network: ChannelPair<NetworkControlMessage, network::Report>,
}

/// A pair of channel ends for passing messages to and from another part of the program.
#[derive(Debug)]
pub struct ChannelPair<S, R> {
    pub sender: Sender<S>,
    pub receiver: Receiver<R>,
}

/// An error indicating the reason [Executor::run] closed unexpectedly.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    kind: ErrorKind,
}

/// The precise reason for the unexpected exit.
#[derive(Clone, Debug, PartialEq)]
pub enum ErrorKind {
    /// [Executor] could not communicate with the UI.
    UiClosed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ErrorKind::*;
        match self.kind {
            UiClosed => write!(f, "lost connection to UI"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq)]
enum RunStatus {
    Continue,
    Exit(Result<(), Error>),
}

impl Executor {
    /// Initializes an [Executor] that's ready to process messages on the provided channels.
    #[allow(unused_variables)]
    pub fn new(logger: ExecutiveLog) -> (Self, ui::ChannelPair, network::ChannelPair) {
        let (ui_send, exec_ui_recv) = channel::unbounded();
        let (exec_ui_send, ui_recv) = channel::unbounded();

        // Return these for UI's use.
        let to_ui = ui::ChannelPair {
            sender: ui_send,
            receiver: ui_recv,
        };

        // For Executor::ui.
        let ui = ChannelPair {
            sender: exec_ui_send,
            receiver: exec_ui_recv,
        };

        let (net_send, exec_net_recv) = channel::unbounded();
        let (exec_net_send, net_recv) = channel::unbounded();

        // Return these for network's use.
        let to_network = network::ChannelPair {
            sender: net_send,
            receiver: net_recv,
        };

        // For Executor::network.
        let network = ChannelPair {
            sender: exec_net_send,
            receiver: exec_net_recv,
        };

        let executor = Executor {
            ui,
            logger,
            network,
        };

        (executor, to_ui, to_network)
    }

    /// Starts processing messages and handling program logic.
    ///
    /// Blocks until the program is getting ready to exit. You will probably wish to do something
    /// like spawn a thread to run this method.
    #[allow(clippy::result_unit_err)]
    pub fn run(mut self) -> Result<(), Error> {
        let mut host_plans = HashMap::new();
        let mut ignored_hosts = HashSet::new();
        loop {
            if let RunStatus::Exit(result) = self._run_once(&mut host_plans, &mut ignored_hosts) {
                return result;
            }
        }
    }

    /// A single iteration of the [Self::run] loop, broken out for better testing.
    ///
    /// Specifically, this method allows for step-by-step evaluation and guarantees termination.
    ///
    /// # host_plans
    ///
    /// For efficiency, we use a slightly complex data structure to store work. `host_plans` maps
    /// hosts to queues of [HostPlanIntoIter] values. This data structure allows us to keep all
    /// hosts busy rather than hitting blocks periodically when running multiple [Plan]s.
    ///
    /// If we were to go through one [Plan] completely before proceeding with the next, then the
    /// slowest host to run the [Plan] would block the rest.
    ///
    /// Instead, when we receive a [Plan], we can generate all applicable [HostPlanIntoIter] values
    /// and enqueue them for processing.
    ///
    /// # Returns
    ///
    /// Returns whether to continue looping, plus a return value if exiting.
    fn _run_once(
        &mut self,
        host_plans: &mut HashMap<String, VecDeque<HostPlanIntoIter>>,
        ignored_hosts: &mut HashSet<String>,
    ) -> RunStatus {
        // For a detailed discussion of how this kind of event loop is designed, see
        // crate::reference::network::Network::_run_once.

        // Prioritize messages from the UI, since they represent the user's intent.
        match self.ui.receiver.try_recv() {
            Ok(ui::Message::RunPlan(plan)) => {
                // For each host in the Plan, either enqueue the Plan or, if it's a new host, start
                // a new queue and run the first HostAction.
                for host in plan.hosts() {
                    use std::collections::hash_map::Entry::*;
                    match host_plans.entry(host.clone()) {
                        Occupied(mut entry) => {
                            // Existing. Simply enqueue. The host is already busy.
                            //
                            // If unwrap panics, then there's a bug somewhere in crate::core,
                            // because plan.hosts() returned a list that included this host.
                            let iter = plan.plan_for(&host).unwrap().into_iter();
                            entry.get_mut().push_back(iter);
                        }
                        Vacant(entry) => {
                            // New host. Send out the first HostAction, and then enqueue the
                            // iterator for future use.
                            //
                            // If unwrap panics, then there's a bug somewhere in crate::core,
                            // because plan.hosts() returned a list that included this host.
                            let mut iter = plan.plan_for(&host).unwrap().into_iter();

                            // If there isn't at least one HostAction in the iterator, then there's
                            // a bug somewhere in crate::core, because plan.hosts() returned a list
                            // that included this host.
                            let host_action = iter.next().unwrap();

                            // Inform the network so it can contact the host. If the channel to the
                            // network is closed, the program is crashing, so we need to panic.
                            let message = NetworkControlMessage::RunAction(host_action);
                            self.network.sender.send(message).unwrap();

                            // Enqueue the iterator.
                            let mut queue = VecDeque::new();
                            queue.push_back(iter);
                            entry.insert(queue);
                        }
                    }
                }
                return RunStatus::Continue;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // The UI has closed or crashed. This means we exit. Whether we exit with success
                // or failure depends on whether we were idle.

                if host_plans.len() > 0 {
                    return RunStatus::Exit(Err(Error {
                        kind: ErrorKind::UiClosed,
                    }));
                }
                return RunStatus::Exit(Ok(()));
            }
        }

        use network::Report::*;
        // If there are no messages from the UI, then check for reports from the network.

        let maybe_report = self.network.receiver.try_recv();

        // First, pass any received reports to the logger and the UI.
        if let Ok(ref report) = maybe_report {
            let report = Report::NetworkReport(report.clone());
            // TODO Consider making this method return an error instead of panicking. Then add an
            // ErrorKind variant, e.g. LogDisconnected.
            self.logger.report(report.clone());

            if let Err(_) = self.ui.sender.send(report) {
                return RunStatus::Exit(Err(Error {
                    kind: ErrorKind::UiClosed,
                }));
            }
        }

        // Finally, process the report, as well as any channel errors.
        match maybe_report {
            Ok(Connecting(_)) | Ok(Connected(_)) | Ok(RunningAction { .. }) => {
                // This is just a status update. No actions needed.
            }
            Ok(FailedToConnect { host, error })
            | Ok(Disconnected {
                host,
                error: Some(error),
            }) => {
                // Proceed with the run, but assume that this host is inaccessible.
                self.ignore_host(host_plans, ignored_hosts, &host);
            }
            Ok(Disconnected { host, error: None }) => {
                // If we had plans for the host, then it was disconnected while it had work to do.
                // Proceed the same way as FailedToConnect.
                //
                // Otherwise, this is not an error condition. No actions necessary.
                if host_plans.contains_key(&host) {
                    self.ignore_host(host_plans, ignored_hosts, &host);
                }
            }
            Ok(ActionResult {
                host,
                manifest_source,
                manifest_name,
                task_source,
                task_name,
                action,
                result,
            }) => {
                if result.is_err() {
                    // Error state.
                    todo!()
                } else if result.as_ref().unwrap().status.success() {
                    // Everything is fine. Report this event and send along the next HostAction.
                    // If there are no most HostActions for this host, then tell the network to
                    // disconnect and remove the host from host_plans.
                    todo!()
                } else {
                    // We have a Result::Ok(Output), but Output indicates an error. Error state.
                    todo!()
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // The network is gone. This should never happen. We must panic.
            }
        }

        // Wait for either Receiver to be ready, then try again.
        RunStatus::Continue
    }

    fn ignore_host(
        &self,
        host_plans: &mut HashMap<String, VecDeque<HostPlanIntoIter>>,
        ignored_hosts: &mut HashSet<String>,
        host: &str,
    ) {
        // Remove any existing plans for this host. In the event that the network isn't
        // perfectly behaved, ignore any errors trying to remove the host, as we can't
        // know precisely what's going on and we can keep our own data in a valid state.
        if host_plans.remove(host).is_none() {
            self.logger.warning(format!(
                "Tried to clear host \"{}\" from Executor's state, but could not find it",
                host
            ));
        }

        // Ignore this host if it comes up in the future.
        ignored_hosts.insert(host.to_string());
    }
}

/// Instructions that [Executor] sends to the [network] to control its operation.
///
/// The [network] will commonly respond with one or more [network::Report] messages as it follows
/// these instructions.
#[derive(Clone, Debug, PartialEq)]
pub enum NetworkControlMessage {
    /// Instructs the [network] to run an [Action] on a specific host.
    RunAction(Arc<HostAction>),

    /// Instructs the [network] to disconnect from the specified host.
    ///
    /// This is typically sent after [Executor] receives a [network::Report::ActionResult] for the
    /// last [Action] that [Executor] needs to run on a given host.
    Disconnect(String),
}

/// Status updates sent to the [ui] and [logger].
// TODO Implement std::fmt::Display.
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// There's no more work to do; the program is now either idle or finished, depending on the
    /// UI's program flow.
    Done,

    /// Pass through a report from the network.
    NetworkReport(network::Report),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixtures::plan;
    use crate::core::Action;
    use crate::logger::LogEntry;
    use crate::ui;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::process::Output;

    mod new {
        use super::*;

        #[test]
        fn uses_provided_logger() {
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (executor, _ui, _network) = Executor::new(logger);

            // Send a Report and a raw entry.
            executor.logger.report(Report::Done);
            executor.logger.notice("entry".into());

            let received_report = report_receiver.try_recv();
            let received_notice = raw_receiver.try_recv();

            assert!(
                matches!(received_report, Ok(LogEntry::Notice(Report::Done))),
                "Received {:?}",
                received_report
            );

            assert!(
                matches!(received_notice, Ok(LogEntry::Notice(_))),
                "Received {:?}",
                received_report
            );
        }

        #[test]
        fn pairs_with_ui() {
            let (logger, _report_receiver, _raw_receiver) = ExecutiveLog::fixture();
            let (executor, ui, _network) = Executor::new(logger);
            let (plan, _, _, _) = plan();

            ui.sender.try_send(ui::Message::RunPlan(plan)).unwrap();

            let received = executor.ui.receiver.try_recv();

            assert!(
                matches!(received, Ok(ui::Message::RunPlan(_))),
                "Received {:?}",
                received
            );
        }

        #[test]
        fn pairs_with_network() {
            let (logger, _report_receiver, _raw_receiver) = ExecutiveLog::fixture();
            let (executor, _ui, network) = Executor::new(logger);

            network
                .sender
                .try_send(network::Report::Connecting("host".into()))
                .unwrap();

            let received = executor.network.receiver.try_recv();

            assert!(
                matches!(received, Ok(network::Report::Connecting(_))),
                "Received {:?}",
                received
            );
        }
    }

    mod _run_once {
        use super::*;

        #[test]
        fn prioritizes_ui_messages() {
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);

            // Send both UI and network messages.
            let (plan, _, _, _) = plan();
            ui.sender.try_send(ui::Message::RunPlan(plan)).unwrap();
            network
                .sender
                .try_send(network::Report::Connecting("host".into()))
                .unwrap();

            let _ = executor._run_once(&mut HashMap::new(), &mut HashSet::new());

            // Verify that the UI message was retrieved and the network message was not.
            assert_eq!(Err(TryRecvError::Empty), executor.ui.receiver.try_recv());
            assert_eq!(
                Ok(network::Report::Connecting("host".into())),
                executor.network.receiver.try_recv()
            );
        }

        mod with_ui_run_plan {
            use super::*;

            #[test]
            fn enqueues_plan_for_existing_host_and_continues() {
                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
                let mut ignored_hosts: HashSet<String> = HashSet::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
                // ui::Message for easier use.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();
                let message = ui::Message::RunPlan(plan);

                // Prepare host_plans by asking the code under test to run a Plan.
                ui.sender.try_send(message.clone()).unwrap();
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );
                assert_eq!(1, host_plans[&host].len());

                // Send the same Plan and run the code again. Verify that the queue for the Plan's
                // host lengthened to 2.
                ui.sender.try_send(message).unwrap();
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );
                assert_eq!(2, host_plans[&host].len());
            }

            #[test]
            fn runs_action_and_enqueues_plan_for_new_host_and_continues() {
                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
                let mut ignored_hosts: HashSet<String> = HashSet::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
                // ui::Message for easier use.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();
                let message = ui::Message::RunPlan(plan);

                // Send the Plan and run the code under test.
                ui.sender.try_send(message).unwrap();
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );

                // Verify that the network received the right message.
                let ncm = network.receiver.try_recv();
                match ncm {
                    Ok(NetworkControlMessage::RunAction(host_action)) => {
                        assert_eq!(host, host_action.host());
                    }
                    message => panic!("Received {:?}", message),
                }

                // Verify that an iterator was enqueued for the host.
                assert_eq!(1, host_plans[&host].len());
            }

            #[test]
            fn processes_all_hosts_and_continues() {
                // We could simply add a second host to runs_action_and_enqueues_plan_for_new_host,
                // but the better approach is to use this test to check a scenario where some hosts
                // are existing and others are new. We'll have two hosts of each kind, and they'll
                // alternate.

                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
                let mut ignored_hosts: HashSet<String> = HashSet::new();

                // Generate the Plan, verify that it has exactly one Manifest, and override that
                // Manifest's hosts so we have two of our four hosts in the first run.
                let (mut plan, _, _, _) = plan();
                plan.manifests.truncate(1);
                plan.manifests[0].hosts = vec!["Existing 1".into(), "Existing 2".into()];
                let message = ui::Message::RunPlan(plan.clone());

                // Send the Plan and run the code under test to populate the two "existing" hosts.
                // Verify invariants for testing sanity.
                ui.sender.try_send(message).unwrap();
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );
                assert_eq!(2, host_plans.len());

                // Override the hosts to intersperse two new entries. Send it to the code under
                // test and run it again.
                // Run the code under test.
                plan.manifests[0].hosts = vec![
                    "Existing 1".into(),
                    "New 1".into(),
                    "Existing 2".into(),
                    "New 2".into(),
                ];
                let message = ui::Message::RunPlan(plan);
                ui.sender.try_send(message).unwrap();
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );
                assert_eq!(4, host_plans.len());

                // Verify that the network received 4 messages (rather than, say, 6).
                assert_eq!(4, network.receiver.try_iter().count());
            }
        }

        #[test]
        fn exits_with_error_if_active() {
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            // The Plan we'll use ships with at least one host. (At time of writing, it has
            // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
            // ui::Message for easier use.
            let (plan, _, _, _) = plan();
            let host = plan.hosts()[0].clone();
            let message = ui::Message::RunPlan(plan);

            // Prepare host_plans by asking the code under test to run a Plan.
            ui.sender.try_send(message.clone()).unwrap();
            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );
            assert_eq!(1, host_plans[&host].len());

            // Simulate the UI closing.
            drop(ui);

            assert_eq!(
                RunStatus::Exit(Err(Error {
                    kind: ErrorKind::UiClosed
                })),
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );
        }

        #[test]
        fn exits_ok_if_idle() {
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            // The Plan we'll use ships with at least one host. (At time of writing, it has
            // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
            // ui::Message for easier use.
            let (plan, _, _, _) = plan();
            let host = plan.hosts()[0].clone();
            let message = ui::Message::RunPlan(plan);

            // Simulate the UI closing.
            drop(ui);

            assert_eq!(
                RunStatus::Exit(Ok(())),
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );
        }

        #[test]
        fn logs_all_reports() {
            use network::Report::*;
            let reports = [
                Connecting("host".into()),
                Connected("host".into()),
                RunningAction {
                    host: "host".to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                },
                FailedToConnect {
                    host: "host".to_string(),
                    error: "error".to_string(),
                },
                Disconnected {
                    host: "host".to_string(),
                    error: Some("error".to_string()),
                },
                Disconnected {
                    host: "host".to_string(),
                    error: None,
                },
                ActionResult {
                    host: "host".to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                    result: Ok(Output {
                        status: ExitStatus::from_raw(0),
                        stdout: "Success".into(),
                        stderr: "".into(),
                    }),
                },
            ];

            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            for report in &reports {
                network.sender.send(report.clone()).unwrap();
            }

            for report in &reports {
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );
            }

            assert_eq!(reports.len(), report_receiver.try_iter().count());
        }

        #[test]
        fn passes_all_reports_on_to_ui() {
            use network::Report::*;
            let reports = [
                Connecting("host".into()),
                Connected("host".into()),
                RunningAction {
                    host: "host".to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                },
                FailedToConnect {
                    host: "host".to_string(),
                    error: "error".to_string(),
                },
                Disconnected {
                    host: "host".to_string(),
                    error: Some("error".to_string()),
                },
                Disconnected {
                    host: "host".to_string(),
                    error: None,
                },
                ActionResult {
                    host: "host".to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                    result: Ok(Output {
                        status: ExitStatus::from_raw(0),
                        stdout: "Success".into(),
                        stderr: "".into(),
                    }),
                },
            ];

            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            for report in &reports {
                network.sender.send(report.clone()).unwrap();
            }

            for report in &reports {
                assert_eq!(
                    RunStatus::Continue,
                    executor._run_once(&mut host_plans, &mut ignored_hosts)
                );
            }

            assert_eq!(reports.len(), ui.receiver.try_iter().count());
        }

        #[test]
        fn connecting_continues() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();
            network.sender.send(Connecting("host".into())).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );
        }

        #[test]
        fn connected_continues() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();
            network.sender.send(Connected("host".into())).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );
        }

        #[test]
        fn running_action_continues() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();
            let report = RunningAction {
                host: "host".to_string(),
                manifest_source: Some("manifest".to_string()),
                manifest_name: "mname".to_string(),
                task_source: Some("task".to_string()),
                task_name: "tname".to_string(),
                action: Arc::new(Action::Shell {
                    commands: vec!["pwd".to_string()],
                }),
            };
            network.sender.send(report).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );
        }

        #[test]
        fn failed_to_connect_clears_and_ignores_host() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            // Pre-populate host_plans so there's an entry to clear.
            const HOST: &str = "host";
            let old_queue = host_plans.insert(HOST.to_string(), VecDeque::new());
            assert!(old_queue.is_none());

            let report = FailedToConnect {
                host: HOST.to_string(),
                error: "error".to_string(),
            };
            network.sender.send(report).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );

            assert_eq!(0, host_plans.len());
            assert!(ignored_hosts.contains(HOST));
        }

        #[test]
        fn failed_to_connect_logs_error_if_host_not_found() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();
            const HOST: &str = "host";

            // For this test, we deliberately don't pre-populate host_plans.

            let report = FailedToConnect {
                host: HOST.to_string(),
                error: "error".to_string(),
            };
            network.sender.send(report).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );

            assert_eq!(0, host_plans.len());
            assert!(ignored_hosts.contains(HOST));

            assert_eq!(
                Ok(LogEntry::Warning(format!(
                    "Tried to clear host \"{}\" from Executor's state, but could not find it",
                    HOST
                ))),
                raw_receiver.try_recv()
            );
        }

        #[test]
        fn disconnected_with_error_clears_and_ignores_host() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            // Pre-populate host_plans so there's an entry to clear.
            const HOST: &str = "host";
            let old_queue = host_plans.insert(HOST.to_string(), VecDeque::new());
            assert!(old_queue.is_none());

            let report = Disconnected {
                host: HOST.to_string(),
                error: Some("error".to_string()),
            };
            network.sender.send(report).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );

            assert_eq!(0, host_plans.len());
            assert!(ignored_hosts.contains(HOST));
        }

        #[test]
        fn disconnected_with_error_logs_error_if_host_not_found() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();
            const HOST: &str = "host";

            // For this test, we deliberately don't pre-populate host_plans.

            let report = Disconnected {
                host: HOST.to_string(),
                error: Some("error".to_string()),
            };
            network.sender.send(report).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );

            assert_eq!(0, host_plans.len());
            assert!(ignored_hosts.contains(HOST));

            assert_eq!(
                Ok(LogEntry::Warning(format!(
                    "Tried to clear host \"{}\" from Executor's state, but could not find it",
                    HOST
                ))),
                raw_receiver.try_recv()
            );
        }

        #[test]
        fn disconnected_without_error_unexpectedly_clears_and_ignores_host() {
            use network::Report::*;
            let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
            let (mut executor, ui, network) = Executor::new(logger);
            let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
            let mut ignored_hosts: HashSet<String> = HashSet::new();

            // Pre-populate host_plans so there's an entry to clear.
            const HOST: &str = "host";
            let old_queue = host_plans.insert(HOST.to_string(), VecDeque::new());
            assert!(old_queue.is_none());

            let report = Disconnected {
                host: HOST.to_string(),
                error: None,
            };
            network.sender.send(report).unwrap();

            assert_eq!(
                RunStatus::Continue,
                executor._run_once(&mut host_plans, &mut ignored_hosts)
            );

            assert_eq!(0, host_plans.len());
            assert!(ignored_hosts.contains(HOST));
        }

        // TODO Test everything below disconnected.

        // TODO Ignore hosts on the ignore list and test this.
    }
}
