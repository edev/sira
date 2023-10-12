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
use crossbeam::channel::{self, Receiver, Select, Sender, TryRecvError};
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
            let mut select = Select::new();
            select.recv(&self.ui.receiver);
            select.recv(&self.network.receiver);
            select.ready();

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
                return self.process_run_plan(host_plans, ignored_hosts, plan);
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
        if let Some(retval) = self.process_report(host_plans, ignored_hosts, maybe_report) {
            return retval;
        }

        // Wait for either Receiver to be ready, then try again.
        RunStatus::Continue
    }

    fn process_run_plan(
        &self,
        host_plans: &mut HashMap<String, VecDeque<HostPlanIntoIter>>,
        ignored_hosts: &mut HashSet<String>,
        plan: Plan,
    ) -> RunStatus {
        // For each host in the Plan, either enqueue the Plan or, if it's a new host, start
        // a new queue and run the first HostAction.
        for host in plan.hosts() {
            if ignored_hosts.contains(&host) {
                continue;
            }

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
        RunStatus::Continue
    }

    /// Private helper for processing a [network::Report]. Used by [Self::_run_once].
    ///
    /// # Returns
    ///
    /// If [None], the calling code should continue executing in its normal control flow.
    /// If [Some], the calling code should return the contained value.
    fn process_report(
        &self,
        host_plans: &mut HashMap<String, VecDeque<HostPlanIntoIter>>,
        ignored_hosts: &mut HashSet<String>,
        maybe_report: Result<network::Report, TryRecvError>,
    ) -> Option<RunStatus> {
        use network::Report::*;
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
                ref host,
                ref result,
                ..
            }) => {
                if result.is_err() || (result.is_ok() && !result.as_ref().unwrap().status.success())
                {
                    // This host encountered an error but is still connected and waiting our
                    // instructions. Either result was an Err or result was an Ok(output) but
                    // output.success() returned false.
                    //
                    // Tell the host to disconnect, and then ignore it. The
                    // network::Report::Disconnected that should come in will pass through the
                    // _run_once logic harmlessly, just as if the host had no more work to do.
                    //
                    // If send fails, panic, because executor's network channels should never close.
                    self.network
                        .sender
                        .send(NetworkControlMessage::Disconnect(host.clone()))
                        .unwrap();
                    self.ignore_host(host_plans, ignored_hosts, &host);
                } else {
                    // Everything is fine. Send along the next HostAction. If there are no more
                    // HostActions for this host, then tell the network to disconnect and remove
                    // the host from host_plans.

                    // If we're operating on an ignored host, this suggests either a logic error or
                    // a protocol design flaw. It's not particularly serious, so we won't panic,
                    // but we'll panic during testing to catch it.
                    debug_assert!(
                        !ignored_hosts.contains(host),
                        "Received an ActionResult from an ignored host:\n{:?}",
                        maybe_report.unwrap()
                    );
                    // First, pull a mutable reference to the host's queue.
                    let queue = match host_plans.get_mut(host) {
                        Some(queue) => queue,
                        None => {
                            // There's no matching queue, which shouldn't be possible. There's
                            // a clear, safe way to proceed, so no need to panic, but we'll log
                            // a warning for troubleshooting.
                            self.logger.warning(format!(
                                "Received an ActionResult for host \"{}\" but couldn't find a \
                                queue for this host",
                                host
                            ));
                            return None;
                        }
                    };

                    // Pull from the next iterator, or discard it and try the one afterward. Keep
                    // going until we have a HostAction or we're out of iterators.
                    let mut next_action = Self::next_action(queue);
                    while next_action.is_none() && !queue.is_empty() {
                        let _ = queue.pop_front();
                        next_action = Self::next_action(queue);
                    }

                    // Either we've found the next HostAction or we're done with this host.
                    //
                    // Panic if we can't reach the network, as this should never happen.
                    match next_action {
                        Some(host_action) => {
                            self.network
                                .sender
                                .send(NetworkControlMessage::RunAction(host_action))
                                .unwrap();
                        }
                        None => {
                            // The queue is empty, so we also need to remove the mapping from
                            // host_plans.
                            let _ = host_plans.remove(host);
                            self.network
                                .sender
                                .send(NetworkControlMessage::Disconnect(host.clone()))
                                .unwrap();

                            // If the queue is empty, the whole data structure might be empty, too.
                            //
                            // If we can't reach the UI, exit with success, since the system is
                            // idle anyway.
                            if host_plans.is_empty() {
                                self.logger.report(Report::Done);
                                if self.ui.sender.send(Report::Done).is_err() {
                                    return Some(RunStatus::Exit(Ok(())));
                                }
                            }
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // The network is gone. This should never happen. We must panic.
                panic!(
                    "Could not receive messages from the network because it disconnected. \
                    Please report this bug!"
                );
            }
        }
        None
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

    fn next_action(queue: &mut VecDeque<HostPlanIntoIter>) -> Option<Arc<HostAction>> {
        match queue.front_mut() {
            Some(into_iter) => into_iter.next(),
            None => None,
        }
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

        /// Contains all of the values that tests in this module commonly require.
        ///
        /// Meant to ease setup and help write DRY tests. There are so many values that
        /// many/most/all tests need that it just makes more sense to structure them rather than
        /// initializing over half a dozen variables via copied code in each test. This wastes a
        /// bit of CPU time, but the tests are still instantaneous.
        struct Fixture {
            /// The bare Receiver for executor::Report values that the logging system would read.
            report_receiver: Receiver<LogEntry<Report>>,

            /// The bare Receiver for arbitrary String values that the logging system would read.
            raw_receiver: Receiver<LogEntry<String>>,

            /// The Executor that on which you'll call the code under test.
            executor: Executor,

            /// The channels that the UI uses to talk with the Executor.
            ui: ui::ChannelPair,

            /// The channels that the network uses to talk with the Executor.
            network: network::ChannelPair,

            /// An empty collection suitable for passing to _run_once as host_plans.
            host_plans: HashMap<String, VecDeque<HostPlanIntoIter>>,

            /// An empty collection suitable for passing to _run_once as ignored_hosts.
            ignored_hosts: HashSet<String>,

            /// A common host to use when generating value; this is compile-time static.
            host: &'static str,
        }

        /// Generates a RunFixture and returns it.
        impl Fixture {
            fn new() -> Self {
                let (logger, report_receiver, raw_receiver) = ExecutiveLog::fixture();
                let (mut executor, ui, network) = Executor::new(logger);
                let mut host_plans: HashMap<String, VecDeque<HostPlanIntoIter>> = HashMap::new();
                let mut ignored_hosts: HashSet<String> = HashSet::new();

                Fixture {
                    report_receiver,
                    raw_receiver,
                    executor,
                    ui,
                    network,
                    host_plans,
                    ignored_hosts,
                    host: "host",
                }
            }

            fn send_from_ui(&self, plan: Plan) {
                self.ui.sender.try_send(ui::Message::RunPlan(plan)).unwrap();
            }

            fn send_from_network(&self, report: network::Report) {
                self.network.sender.try_send(report).unwrap();
            }

            fn network_expects(&self, expected: Result<NetworkControlMessage, TryRecvError>) {
                let received = self.network.receiver.try_recv();
                assert_eq!(
                    expected, received,
                    "Expected {expected:?} but received {received:?}"
                );
            }

            /// Calls _run_once, pulling arguments from the fixture itself, and asserts that it
            /// exits with RunStatus::Continue.
            fn runs_and_continues(&mut self) {
                assert_eq!(
                    RunStatus::Continue,
                    self.executor
                        ._run_once(&mut self.host_plans, &mut self.ignored_hosts)
                );
            }

            fn insert_host_queue(&mut self, queue: VecDeque<HostPlanIntoIter>) {
                let old_queue = self.host_plans.insert(self.host.to_string(), queue);
                assert!(old_queue.is_none());
            }
        }

        #[test]
        fn prioritizes_ui_messages() {
            let mut fixture = Fixture::new();
            let (plan, _, _, _) = plan();

            // Send both UI and network messages.
            fixture.send_from_ui(plan);
            fixture.send_from_network(network::Report::Connecting("host".into()));

            fixture.runs_and_continues();

            // Verify that the UI message was retrieved and the network message was not.
            assert_eq!(
                Err(TryRecvError::Empty),
                fixture.executor.ui.receiver.try_recv()
            );
            assert_eq!(
                Ok(network::Report::Connecting("host".into())),
                fixture.executor.network.receiver.try_recv()
            );
        }

        mod with_ui_run_plan {
            use super::*;

            #[test]
            fn respects_ignored_hosts() {
                // This test verifies specifically that the code under test skips generating
                // HostPlans for hosts on the ignore list. There are also assertions sprinkled
                // elsewhere in code to check the invariant that a host being processed is not
                // on the ignore list; this test does not cover those assertions.

                let mut fixture = Fixture::new();

                // Generate a Plan with one host and ignore it. Then send the Plan to Executor.
                let (mut plan, _, _, _) = plan();
                plan.manifests[0].hosts.truncate(1);
                fixture
                    .ignored_hosts
                    .insert(plan.manifests[0].hosts[0].clone());
                fixture.send_from_ui(plan);

                fixture.runs_and_continues();

                assert_eq!(1, fixture.ignored_hosts.len());
                assert_eq!(0, fixture.host_plans.len());
            }

            #[test]
            fn enqueues_plan_for_existing_host_and_continues() {
                let mut fixture = Fixture::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan and clone the host.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();

                // Prepare host_plans by asking the code under test to run a Plan.
                fixture.send_from_ui(plan.clone());
                fixture.runs_and_continues();
                assert_eq!(1, fixture.host_plans[&host].len());

                // Send the same Plan and run the code again. Verify that the queue for the Plan's
                // host lengthened to 2.
                fixture.send_from_ui(plan);
                fixture.runs_and_continues();
                assert_eq!(2, fixture.host_plans[&host].len());
            }

            #[test]
            fn runs_action_and_enqueues_plan_for_new_host_and_continues() {
                let mut fixture = Fixture::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan and clone the host.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();

                // Send the Plan and run the code under test.
                fixture.send_from_ui(plan);
                fixture.runs_and_continues();

                // Verify that the network received the right message.
                let ncm = fixture.network.receiver.try_recv();
                match ncm {
                    Ok(NetworkControlMessage::RunAction(host_action)) => {
                        assert_eq!(host, host_action.host());
                    }
                    message => panic!("Received {:?}", message),
                }

                // Verify that an iterator was enqueued for the host.
                assert_eq!(1, fixture.host_plans[&host].len());
            }

            #[test]
            fn processes_all_hosts_and_continues() {
                // We could simply add a second host to runs_action_and_enqueues_plan_for_new_host,
                // but the better approach is to use this test to check a scenario where some hosts
                // are existing and others are new. We'll have two hosts of each kind, and they'll
                // alternate.

                let mut fixture = Fixture::new();

                // Generate the Plan, verify that it has exactly one Manifest, and override that
                // Manifest's hosts so we have two of our four hosts in the first run.
                let (mut plan, _, _, _) = plan();
                plan.manifests.truncate(1);
                plan.manifests[0].hosts = vec!["Existing 1".into(), "Existing 2".into()];

                // Send the Plan and run the code under test to populate the two "existing" hosts.
                // Verify invariants for testing sanity.
                fixture.send_from_ui(plan.clone());
                fixture.runs_and_continues();
                assert_eq!(2, fixture.host_plans.len());

                // Override the hosts to intersperse two new entries. Send it to the code under
                // test and run it again.
                plan.manifests[0].hosts = vec![
                    "Existing 1".into(),
                    "New 1".into(),
                    "Existing 2".into(),
                    "New 2".into(),
                ];
                fixture.send_from_ui(plan);
                fixture.runs_and_continues();
                assert_eq!(4, fixture.host_plans.len());

                // Verify that the network received 4 messages (rather than, say, 6).
                assert_eq!(4, fixture.network.receiver.try_iter().count());
            }

            #[test]
            fn exits_with_error_if_active() {
                let mut fixture = Fixture::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan and clone the host.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();

                // Prepare host_plans by asking the code under test to run a Plan.
                fixture.send_from_ui(plan);
                fixture.runs_and_continues();
                assert_eq!(1, fixture.host_plans[&host].len());

                // Simulate the UI closing.
                drop(fixture.ui);

                assert_eq!(
                    RunStatus::Exit(Err(Error {
                        kind: ErrorKind::UiClosed
                    })),
                    fixture
                        .executor
                        ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts)
                );
            }

            #[test]
            fn exits_ok_if_idle() {
                let mut fixture = Fixture::new();

                // The Plan we'll use ships with at least one host. (At time of writing, it has
                // exactly one host.) Generate the Plan, clone the host, and package the Plan in a
                // ui::Message for easier use.
                let (plan, _, _, _) = plan();
                let host = plan.hosts()[0].clone();
                let message = ui::Message::RunPlan(plan);

                // Simulate the UI closing.
                drop(fixture.ui);

                assert_eq!(
                    RunStatus::Exit(Ok(())),
                    fixture
                        .executor
                        ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts)
                );
            }
        }

        mod with_network_report {
            use super::*;

            /// Returns reports of all types in a reasonable order and with reasonable values.
            fn reports() -> Vec<network::Report> {
                use network::Report::*;
                vec![
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
                        // We must use a different host name here so we aren't sending a message
                        // from an ignored host.
                        host: "other_host".to_string(),
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
                ]
            }

            #[test]
            fn logs_all_reports() {
                let reports = reports();
                let mut fixture = Fixture::new();

                // First, send all reports. This is done in a separate loop from running the code
                // under test so that the code under test has every opportunity to misbehave when
                // given a lengthy message queue.
                for report in &reports {
                    fixture.send_from_network(report.clone());
                }

                for report in &reports {
                    fixture.runs_and_continues();
                }

                assert_eq!(reports.len(), fixture.report_receiver.try_iter().count());
            }

            #[test]
            fn passes_all_reports_on_to_ui() {
                let reports = reports();
                let mut fixture = Fixture::new();

                // See note in logs_all_reports.
                for report in &reports {
                    fixture.send_from_network(report.clone());
                }

                for report in &reports {
                    fixture.runs_and_continues();
                }

                assert_eq!(reports.len(), fixture.ui.receiver.try_iter().count());
            }

            #[test]
            fn ui_closed_exits_with_error() {
                let mut fixture = Fixture::new();

                // Send a report that will be forwarded to the UI.
                let report = network::Report::Connecting("host".into());
                fixture.send_from_network(report);

                // Close the UI.
                drop(fixture.ui.receiver);

                assert_eq!(
                    RunStatus::Exit(Err(Error {
                        kind: ErrorKind::UiClosed,
                    })),
                    fixture
                        .executor
                        ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts)
                );
            }

            #[test]
            fn connecting_continues() {
                let mut fixture = Fixture::new();
                fixture.send_from_network(network::Report::Connecting("host".into()));
                fixture.runs_and_continues();
            }

            #[test]
            fn connected_continues() {
                let mut fixture = Fixture::new();
                fixture.send_from_network(network::Report::Connected("host".into()));
                fixture.runs_and_continues();
            }

            #[test]
            fn running_action_continues() {
                let mut fixture = Fixture::new();
                let report = network::Report::RunningAction {
                    host: "host".to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                };
                fixture.send_from_network(report);
                fixture.runs_and_continues();
            }

            #[test]
            fn failed_to_connect_clears_and_ignores_host() {
                let mut fixture = Fixture::new();

                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::FailedToConnect {
                    host: fixture.host.to_string(),
                    error: "error".to_string(),
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));
            }

            #[test]
            fn failed_to_connect_logs_error_if_host_not_found() {
                let mut fixture = Fixture::new();

                // For this test, we deliberately don't pre-populate host_plans.

                let report = network::Report::FailedToConnect {
                    host: fixture.host.to_string(),
                    error: "error".to_string(),
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));

                assert_eq!(
                    Ok(LogEntry::Warning(format!(
                        "Tried to clear host \"{}\" from Executor's state, but could not find it",
                        fixture.host
                    ))),
                    fixture.raw_receiver.try_recv()
                );
            }

            #[test]
            fn disconnected_with_error_clears_and_ignores_host() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::Disconnected {
                    host: fixture.host.to_string(),
                    error: Some("error".to_string()),
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));
            }

            #[test]
            fn disconnected_with_error_logs_error_if_host_not_found() {
                let mut fixture = Fixture::new();

                // For this test, we deliberately don't pre-populate host_plans.

                let report = network::Report::Disconnected {
                    host: fixture.host.to_string(),
                    error: Some("error".to_string()),
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));

                assert_eq!(
                    Ok(LogEntry::Warning(format!(
                        "Tried to clear host \"{}\" from Executor's state, but could not find it",
                        fixture.host
                    ))),
                    fixture.raw_receiver.try_recv()
                );
            }

            #[test]
            fn disconnected_without_error_unexpectedly_clears_and_ignores_host() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::Disconnected {
                    host: fixture.host.to_string(),
                    error: None,
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));
            }

            #[test]
            fn action_result_err_disconnects_and_ignores() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                    result: Err("Disconnected".to_string()),
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));

                fixture.network_expects(Ok(NetworkControlMessage::Disconnect(fixture.host.into())));
            }

            #[test]
            #[should_panic(expected = "SendError")]
            fn action_result_err_panics_if_network_closed() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                    result: Err("Disconnected".to_string()),
                };
                fixture.send_from_network(report);

                // Close the network.
                drop(fixture.network.receiver);

                fixture
                    .executor
                    ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts);
            }

            #[test]
            fn action_result_not_success_disconnects_and_ignores() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                    result: Ok(Output {
                        status: ExitStatus::from_raw(-1),
                        stdout: "Not Success".into(),
                        stderr: "".into(),
                    }),
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert_eq!(0, fixture.host_plans.len());
                assert!(fixture.ignored_hosts.contains(fixture.host));

                fixture.network_expects(Ok(NetworkControlMessage::Disconnect(fixture.host.into())));
            }

            #[test]
            #[should_panic(expected = "SendError")]
            fn action_result_not_success_panics_if_network_closed() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell {
                        commands: vec!["pwd".to_string()],
                    }),
                    result: Ok(Output {
                        status: ExitStatus::from_raw(-1),
                        stdout: "Not Success".into(),
                        stderr: "".into(),
                    }),
                };
                fixture.send_from_network(report);

                // Close the network.
                drop(fixture.network.receiver);

                fixture
                    .executor
                    ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts);
            }

            #[test]
            fn action_result_success_sends_next_host_action() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans with an iterator with at least one value left.
                let (mut plan, _, _, _) = plan();
                plan.manifests[0].hosts = vec![fixture.host.to_string()];
                let queue = VecDeque::from([plan.plan_for(fixture.host).unwrap().into_iter()]);
                fixture.insert_host_queue(queue);

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                fixture.runs_and_continues();

                assert!(matches!(
                    fixture.network.receiver.try_recv(),
                    Ok(NetworkControlMessage::RunAction(_))
                ));
            }

            #[test]
            #[should_panic(expected = "SendError")]
            fn action_result_success_panics_if_network_closed() {
                let mut fixture = Fixture::new();

                // Pre-populate host_plans so there's an entry to clear.
                fixture.insert_host_queue(VecDeque::new());

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                // Close the network.
                drop(fixture.network.receiver);

                fixture
                    .executor
                    ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts);
            }

            #[test]
            fn action_result_success_advances_to_next_plan() {
                let mut fixture = Fixture::new();

                // Create a Plan with one Action left.
                let (mut one_action_plan, _, _, _) = plan();
                one_action_plan.manifests.truncate(1);
                let manifest = &mut one_action_plan.manifests[0];
                manifest.hosts.clear();
                manifest.hosts.push(fixture.host.to_string());
                manifest.include.truncate(1);
                manifest.include[0].actions.truncate(1);

                // Create an iterator with no HostActions left, based on the above Plan.
                let mut done_iter = one_action_plan.plan_for(fixture.host).unwrap().into_iter();
                let _ = done_iter.next();

                // Create an iterator with one HostAction left.
                let mut one_action_iter =
                    one_action_plan.plan_for(fixture.host).unwrap().into_iter();

                // Set up the test scenario.
                let queue = VecDeque::from([done_iter, one_action_iter]);
                fixture.insert_host_queue(queue);

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                // Run the code under test.
                fixture.runs_and_continues();

                assert!(matches!(
                    fixture.network.receiver.try_recv(),
                    Ok(NetworkControlMessage::RunAction(_))
                ));
                assert_eq!(1, fixture.host_plans[fixture.host].len());
            }

            #[test]
            fn action_result_success_warns_if_host_not_found() {
                let mut fixture = Fixture::new();

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                // Run the code under test.
                fixture.runs_and_continues();

                assert_eq!(
                    Ok(LogEntry::Warning(format!(
                        "Received an ActionResult for host \"{}\" but couldn't find a \
                        queue for this host",
                        fixture.host
                    ))),
                    fixture.raw_receiver.try_recv(),
                );
            }

            #[test]
            fn action_result_success_disconnects_host_if_no_more_actions() {
                let mut fixture = Fixture::new();

                // Create a Plan with no Actions but with the right host.
                let (mut empty_plan, _, _, _) = plan();
                empty_plan.manifests.truncate(1);
                let manifest = &mut empty_plan.manifests[0];
                manifest.hosts.clear();
                manifest.hosts.push(fixture.host.to_string());
                manifest.include.clear();

                // Create an iterator with no HostActions left, based on the above Plan.
                let mut done_iter = empty_plan.plan_for(fixture.host).unwrap().into_iter();

                // Set up the test scenario.
                let queue = VecDeque::from([done_iter]);
                fixture.insert_host_queue(queue);

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                // Run the code under test.
                fixture.runs_and_continues();

                assert!(matches!(
                    fixture.network.receiver.try_recv(),
                    Ok(NetworkControlMessage::Disconnect(_))
                ));

                assert!(!fixture.host_plans.contains_key(fixture.host));
            }

            #[test]
            fn reports_done_if_host_plans_is_empty() {
                let mut fixture = Fixture::new();

                // Create a Plan with no Actions but with the right host.
                let (mut empty_plan, _, _, _) = plan();
                empty_plan.manifests.truncate(1);
                let manifest = &mut empty_plan.manifests[0];
                manifest.hosts.clear();
                manifest.hosts.push(fixture.host.to_string());
                manifest.include.clear();

                // Create an iterator with no HostActions left, based on the above Plan.
                let mut done_iter = empty_plan.plan_for(fixture.host).unwrap().into_iter();

                // Set up the test scenario.
                let queue = VecDeque::from([done_iter]);
                fixture.insert_host_queue(queue);

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                // Run the code under test.
                fixture.runs_and_continues();

                let logged_reports: Vec<_> = fixture.report_receiver.try_iter().collect();
                assert!(logged_reports.contains(&LogEntry::Notice(Report::Done)));
            }

            #[test]
            fn exits_if_host_plans_is_empty_and_ui_is_closed() {
                // The code under test is specifically the RunStatus::Exit(Ok(()) inside
                // process_report(). This code only runs under a specific race outcome: the UI
                // closes between _run_once() reporting a network::Report::ActionResult and
                // process_report() reaching the code under test. Therefore, this test breaks with
                // the other tests in this section and directly calls process_report() after
                // closing the UI.

                let mut fixture = Fixture::new();

                // Create a Plan with no Actions but with the right host.
                let (mut empty_plan, _, _, _) = plan();
                empty_plan.manifests.truncate(1);
                let manifest = &mut empty_plan.manifests[0];
                manifest.hosts.clear();
                manifest.hosts.push(fixture.host.to_string());
                manifest.include.clear();

                // Create an iterator with no HostActions left, based on the above Plan.
                let mut done_iter = empty_plan.plan_for(fixture.host).unwrap().into_iter();

                // Set up the test scenario.
                let queue = VecDeque::from([done_iter]);
                fixture.insert_host_queue(queue);

                let report = network::Report::ActionResult {
                    host: fixture.host.to_string(),
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
                };
                fixture.send_from_network(report);

                drop(fixture.ui.receiver);

                // Run the code under test.
                let maybe_report = fixture.executor.network.receiver.try_recv();
                assert_eq!(
                    Some(RunStatus::Exit(Ok(()))),
                    fixture.executor.process_report(
                        &mut fixture.host_plans,
                        &mut fixture.ignored_hosts,
                        maybe_report
                    )
                );

                let logged_reports: Vec<_> = fixture.report_receiver.try_iter().collect();
                assert!(logged_reports.contains(&LogEntry::Notice(Report::Done)));
            }

            #[test]
            #[should_panic(expected = "Could not receive messages")]
            fn panics_if_network_disconnected() {
                let mut fixture = Fixture::new();
                drop(fixture.network.sender);
                fixture
                    .executor
                    ._run_once(&mut fixture.host_plans, &mut fixture.ignored_hosts);
            }
        }
    }
}
