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
use crate::ui;
use crossbeam::channel::{Receiver, Sender};
use std::sync::Arc;

/// Coordinates message routing, plan execution, and program flow.
#[allow(dead_code)]
pub struct Executor {
    ui: ChannelPair<UiMessage, ui::Message>,
    logger: ChannelPair<(), ()>,
    network: ChannelPair<(), ()>,
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
        ui: ChannelPair<UiMessage, ui::Message>,
        logger: ChannelPair<(), ()>,
        network: ChannelPair<(), ()>,
    ) -> Self {
        Executor {
            ui,
            logger,
            network,
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

/// Messages that [Executor] may send to when in the [UiState::Plan] state.
///
/// In general, messages sent out to [network interfaces] are also broadcast to the [ui] and
/// [logger], since those parties might be interested in observing these events. Some messages have
/// different behaviors. The behavior of each message is documented below.
///
/// # Standard broadcast
///
/// Many messages are marked as "standard broadcast". This is shorthand for the following:
///
/// * The primary recipient of the message is the [network interface]. Any expected actions will
/// still be documented.
///
/// * The [logger] receives an informational copy and records the event.
///
/// * The [ui] receives an informational copy so that it may update the user's view if desired; no
/// action is required.
///
/// [logger]: crate::logger
/// [network interface]: core::net
#[derive(Debug)]
pub enum Message {
    /// Requests that the [HostAction] be run.
    ///
    /// When the [network interface] receives this, it should compile the [Action] and send it to
    /// `sira-client`. Once the [Action] has finished running, the [network interface] should send
    /// a [crate::net::Message::ActionReport].
    ///
    /// This is a standard broadcast.
    ///
    /// [Action]: crate::core::action::Action
    /// [network interface]: crate::net
    RunAction(Arc<HostAction>),

    /// Requests that the [network interface] disconnect from a host, e.g. because there are no
    /// more [Action]s to run.
    ///
    /// When the [network interface] receives this, it should disconnect from the given host. If it
    /// was not connected to (or aware of) the host, it should ignore this message. This message
    /// has no response in [crate::net::Message].
    ///
    /// This is a standard broadcast.
    ///
    /// [Action]: crate::core::action::Action
    /// [network interface]: crate::net
    Disconnect(String),
}

pub enum UiMessage {}
