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
//! instances of `sira-client`.
//!
//! # Modularity
//!
//! The components that connect to the [Executor] are designed to be swapped out freely if you
//! desire. The [Executor] itself, however, is not replaceable. It provides the core logic that
//! drives program flow and acts as the glue that binds all the modular components together;
//! writing your own [Executor] would essentially be rewriting Sira itself. Thus, this module does
//! not promise to export the types necessary to write a custom [Executor].

use crate::core::action::HostAction;
use crate::core::plan::Plan;
use crate::network;
use crate::ui;
use crossbeam::channel::{Receiver, Sender};
use std::collections::VecDeque;

/// Coordinates message routing, plan execution, and program flow.
#[allow(dead_code)]
pub struct Executor {
    ui: ChannelPair<Report, ui::Message>,
    logger: ChannelPair<(), ()>,
    network: ChannelPair<(), ()>,
    plans: VecDeque<Plan>,
}

/// A pair of channel ends for passing messages to and from another part of the program.
#[derive(Debug)]
pub struct ChannelPair<S, R> {
    pub sender: Sender<S>,
    pub receiver: Receiver<R>,
}

impl Executor {
    // TODO Change this method to new() -> (Self, channel pairs for other components).
    /// Initializes an [Executor] that's ready to process messages on the provided channels.
    pub fn new(
        ui: ChannelPair<Report, ui::Message>,
        logger: ChannelPair<(), ()>,
        network: ChannelPair<(), ()>,
    ) -> Self {
        Executor {
            ui,
            logger,
            network,
            plans: VecDeque::new(),
        }
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

pub enum NetworkControlMessage {
    RunAction(HostAction),
}

/// Status updates sent to the [ui] and [logger].
///
/// [logger]: crate::logger
pub enum Report {
    /// There's no more work to do; the program is now either idle or finished, depending on the
    /// UI's program flow.
    Done,

    /// Pass through a report from the network.
    NetworkReport(network::Report),
}
