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
use std::collections::{HashMap, VecDeque};
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
    pub fn run(mut self) {
        let mut host_plans = HashMap::new();
        loop {
            while self._run_once(&mut host_plans) {}
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
    /// Returns whether to continue looping.
    fn _run_once(&mut self, host_plans: &mut HashMap<String, VecDeque<HostPlanIntoIter>>) -> bool {
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
                return true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // The UI has closed or crashed. This means we exit. Whether we exit with success
                // or failure depends on whether we were idle.
            }
        }

        use network::Report::*;
        // If there are no messages from the UI, then check for reports from the network.
        match self.network.receiver.try_recv() {
            Ok(Connecting(_)) | Ok(Connected(_)) | Ok(RunningAction { .. }) => {
                // This is just a status update. No actions needed; just inform everyone.
                todo!()
            }
            Ok(FailedToConnect { host, error }) => {
                // Error state.
                todo!()
            }
            Ok(Disconnected {
                host,
                error: Some(error),
            }) => {
                // Error state.
                todo!()
            }
            Ok(Disconnected { host, error: None }) => {
                // If the host should have been working, then this is an error state. If this is
                // expected, then it's not.
                todo!()
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
        true
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
#[derive(Debug)]
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
    use crate::logger::LogEntry;
    use crate::ui;

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

            let _ = executor._run_once(&mut HashMap::new());

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
            fn enqueues_plan_for_existing_host_and_returns_true() {
                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
                // ui::Message for easier use.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();
                let message = ui::Message::RunPlan(plan);

                // Prepare host_plans by asking the code under test to run a Plan.
                ui.sender.try_send(message.clone()).unwrap();
                assert!(executor._run_once(&mut host_plans));
                assert_eq!(1, host_plans[&host].len());

                // Send the same Plan and run the code again. Verify that the queue for the Plan's
                // host lengthened to 2.
                ui.sender.try_send(message).unwrap();
                assert!(executor._run_once(&mut host_plans));
                assert_eq!(2, host_plans[&host].len());
            }

            #[test]
            fn runs_action_and_enqueues_plan_for_new_host_and_returns_true() {
                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
                // ui::Message for easier use.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();
                let message = ui::Message::RunPlan(plan);

                // Send the Plan and run the code under test.
                ui.sender.try_send(message).unwrap();
                assert!(executor._run_once(&mut host_plans));

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
            fn processes_all_hosts_and_returns_true() {
                // We could simply add a second host to runs_action_and_enqueues_plan_for_new_host,
                // but the better approach is to use this test to check a scenario where some hosts
                // are existing and others are new. We'll have two hosts of each kind, and they'll
                // alternate.

                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();

                // Generate the Plan, verify that it has exactly one Manifest, and override that
                // Manifest's hosts so we have two of our four hosts in the first run.
                let (mut plan, _, _, _) = plan();
                plan.manifests.truncate(1);
                plan.manifests[0].hosts = vec!["Existing 1".into(), "Existing 2".into()];
                let message = ui::Message::RunPlan(plan.clone());

                // Send the Plan and run the code under test to populate the two "existing" hosts.
                // Verify invariants for testing sanity.
                ui.sender.try_send(message).unwrap();
                assert!(executor._run_once(&mut host_plans));
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
                assert!(executor._run_once(&mut host_plans));
                assert_eq!(4, host_plans.len());

                // Verify that the network received 4 messages (rather than, say, 6).
                assert_eq!(4, network.receiver.try_iter().count());
            }
        }
    }
}
