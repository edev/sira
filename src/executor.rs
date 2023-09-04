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

use crate::core::plan::Plan;
use crate::ui;
use crossbeam::channel::{Receiver, Sender};

/// Coordinates message routing, plan execution, and program flow.
#[allow(dead_code)]
pub struct Executor {
    ui: UiState,
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
    /// Initializes an [Executor] that's ready to process messages on the provided channels.
    pub fn new(ui: UiState, logger: ChannelPair<(), ()>, network: ChannelPair<(), ()>) -> Self {
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

/// Messages that [Executor] may send to the [ui] when in the [UiState::Plan] state.
#[derive(Debug)]
pub enum Message {
    /// Sent when [Executor] is returning to the idle state (no longer executing a plan).
    ///
    /// When the UI receives this, it should switch its [ui::State] to [ui::State::Idle].
    Idle(ui::IdleState),
}

/// Defines the different states of [Executor].
///
/// Internally, each state defines the communication channels that [Executor] and a [ui] may use to
/// communicate with one another.
#[derive(Debug)]
pub enum UiState {
    /// [Executor] is awaiting instructions from the [ui]. No [Plan] is running.
    ///
    /// There is no custom type defining the messaging protocol in this state, because the only
    /// message that the UI may pass is a tuple containing the information that [Executor] needs to
    /// transition to [UiState::Plan].
    ///
    /// # UI
    ///
    /// The [ui] is presumably awaiting user input.
    ///
    /// # Network
    ///
    /// The network is presumably idle or returning to idle.
    ///
    /// # Logger
    ///
    /// The logger is fully active.
    Idle(IdleState),

    /// [Executor] is executing a [Plan].
    ///
    /// # UI
    ///
    /// The [ui] is responding to [Message]s from [Executor] and displaying updates to the user.
    ///
    /// # Network
    ///
    /// The network is executing the [Plan] by following [Executor]'s instructions.
    ///
    /// # Logger
    ///
    /// The logger is fully active.
    Plan(PlanState),
}

/// Defines the [UiState::Idle] state of [Executor].
///
/// Internally, stores the communications channels that [Executor] needs in this state.
#[derive(Debug)]
pub struct IdleState {
    /// Receives all the information that [Executor] needs to switch to [UiState::Plan].
    ///
    /// It is a logic error to receive more than one message from this [Receiver]. As soon as you
    /// receive a message, you must transition to [UiState::Plan] and drop the [IdleState] value.
    /// The [ui] will take similar steps.
    #[allow(dead_code)]
    receiver: Receiver<(Plan, ChannelPair<Message, ui::Message>)>,
}

impl IdleState {
    /// Creates a new [IdleState] value based on a [Receiver].
    ///
    /// This function is meant to be paired with [ui::State::new].
    pub(crate) fn new(receiver: Receiver<(Plan, ChannelPair<Message, ui::Message>)>) -> Self {
        IdleState { receiver }
    }
}

/// Defines the [UiState::Plan] state of [Executor].
///
/// Internally, stores the communications channels that [Executor] needs in this state.
// TODO Decide whether to store the [Plan] in [PlanState] or as an [Option] in [Executor].
pub type PlanState = ChannelPair<Message, ui::Message>;
