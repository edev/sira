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
use crate::core::plan::Plan;
#[cfg(doc)]
use crate::logger;
use crate::logger::ExecutiveLog;
use crate::network;
use crate::ui;
use crossbeam::channel::{self, Receiver, Sender};
use std::collections::VecDeque;
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

    /// Queue of [Plan]s to run. The currently running [Plan] is not in this queue: it has been
    /// dequeued for consumption.
    plans: VecDeque<Plan>,
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
            plans: VecDeque::new(),
        };

        (executor, to_ui, to_network)
    }

    /// Starts processing messages and handling program logic.
    ///
    /// Blocks until the program is getting ready to exit. You will probably wish to do something
    /// like spawn a thread to run this method.
    #[allow(clippy::result_unit_err)]
    pub fn run(self) -> Result<(), ()> {
        loop {
            // TODO Select among receivers and respond accordingly.
            todo!()
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
}
