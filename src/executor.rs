//! The central component of Sira's controller-side software.
//!
//! Provides the communication hub among the user interface, logger, network, and any plans
//! being run. Coordinates the execution of plans on managed nodes.
//!
//! Sira uses an hourglass design: everything communicates through a simple, well-defined interface
//! routed through [Executor], and each component is free to implement any logic or structure
//! within its own scope. The logger may arrange itself in any fashion; the user interface may be
//! simple or complex, text or graphical, etc.; the "network connection" may be SSH (the default),
//! something else, or even some arbitrary non-networked method of connecting to instances of
//! `sira-client`.

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

#[allow(dead_code)]
pub struct Executor {
    ui: ChannelPair<(), ()>,
    logger: ChannelPair<(), ()>,
    network: ChannelPair<(), ()>,
}

#[derive(Debug)]
pub struct ChannelPair<S, R> {
    pub sender: UnboundedSender<S>,
    pub receiver: UnboundedReceiver<R>,
}

impl Executor {
    pub fn new(
        ui: ChannelPair<(), ()>,
        logger: ChannelPair<(), ()>,
        network: ChannelPair<(), ()>,
    ) -> Self {
        Executor {
            ui,
            logger,
            network,
        }
    }

    // pub fn start(self) -> Result<(), ()> {
    //     todo!()
    // }

    // async fn ui_listener(receiver: Receiver<()>, logger: Sender<()>, network: ()) -> Result<(), ()> {
    //     todo!()
    // }
}

// The following is a sketch; it does not necessarily belong in this file, though some or all of it
// might.

use crate::core::plan::Plan;
use tokio::sync::oneshot;

/// A UI's communication channel to [Executor] when there is no currently executing [Plan].
#[derive(Debug)]
pub struct IdleState {
    sender: oneshot::Sender<(Plan, ChannelPair<ExecutorMessage, UiMessage>)>,
}

impl IdleState {
    /// Starts executing a [Plan]. Consumes self and returns a [UiState::Plan].
    pub fn start(self, plan: Plan) -> UiState {
        let (executor_tx, ui_rx) = mpsc::unbounded_channel::<ExecutorMessage>();
        let (ui_tx, executor_rx) = mpsc::unbounded_channel::<UiMessage>();

        let executor_channel_pair = ChannelPair {
            sender: executor_tx,
            receiver: executor_rx,
        };

        self.sender.send((plan, executor_channel_pair)).unwrap();

        UiState::Plan(PlanState {
            sender: ui_tx,
            receiver: ui_rx,
        })
    }
}

pub type PlanState = ChannelPair<UiMessage, ExecutorMessage>;

#[derive(Debug)]
pub enum UiState {
    /// The program is `idle`, meaning it is not executing a [Plan].
    ///
    /// From this state, the UI may start a [Plan] using [IdleState::start].
    Idle(IdleState),

    /// The program is currently executing a [Plan].
    ///
    /// For safety, only one [Plan] may execute at a time. Separating [UiState::Idle] and
    /// [UiState::Plan] is how we accomplish this.
    Plan(PlanState),
}

/// Messages that a UI can send to [Executor].
///
/// Currently there are none, because the planned UI simply monitors a batch job.
#[derive(Debug)]
pub enum UiMessage {}

#[derive(Debug)]
pub enum ExecutorMessage {
    /// Sent when [Executor] is returning to the idle state (no longer executing a plan).
    ///
    /// When the UI receives this, it should switch its [UiState] to [UiState::Idle].
    Idle(IdleState),
}
