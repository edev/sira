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

pub struct IdleState {
    sender: oneshot::Sender<(Plan, ChannelPair<ExecutorMessage, UiMessage>)>,
}

impl IdleState {
    pub fn start(self, plan: Plan) -> PlanState {
        let (executor_tx, ui_rx) = mpsc::unbounded_channel::<ExecutorMessage>();
        let (ui_tx, executor_rx) = mpsc::unbounded_channel::<UiMessage>();

        let executor_channel_pair = ChannelPair {
            sender: executor_tx,
            receiver: executor_rx,
        };

        self.sender.send((plan, executor_channel_pair)).unwrap();

        PlanState {
            sender: ui_tx,
            receiver: ui_rx,
        }
    }
}

pub type PlanState = ChannelPair<UiMessage, ExecutorMessage>;

#[derive(Debug)]
pub enum UiMessage {}

#[derive(Debug)]
pub enum ExecutorMessage {
    /// When [Executor] is listening for [UiMessages] and finds that the UI has dropped the sender
    /// and thus closed the channel, [Executor] infers that the UI wishes to return to the idle
    /// state. Thus, the [Executor] does the following:
    ///
    /// 1. Sends an [ExecutorMessage::IdleState] message to the UI with everything the UI needs to
    ///    return to [IdleState].
    ///
    /// 2. Drops the sender and receiver it was using during plan execution.
    ///
    /// 3. Listens to the idle state receiver paired to the message it just sent to the UI.
    ///
    /// If [Executor] is unable to send the [ExecutorMessage::IdleState] message to the UI
    /// (presumably because the UI has closed its receiver), then [Executor] infers that the UI
    /// wishes to terminate the program.
    ///
    /// NOTE I think it's better to have the UI be more explicit, with both ReturnToIdle and Exit
    /// messages. No inferring, please. In particular, inferring creates a nasty race condition
    /// where Executor can infer the wrong outcome if it tries to send an IdleState message to the
    /// UI, the UI intends to close the channel to indicate that the program is closing, and the
    /// Executor wins the race. Sure, we can cope with this logic, but the much easier and more
    /// obviously correct design is simply to have clear messages indicating intent.
    IdleState(oneshot::Sender<(Plan, ChannelPair<ExecutorMessage, UiMessage>)>),
}

// use crate::core::plan::Plan;
// pub enum IdleMessage {
//     Start(Plan),
// }
//
// enum UiPlanMessage {}
//
// enum ExecutorPlanMessage {}
//
// enum RunningState {
//     Idle(Sender<IdleMessage>),
//     Plan(ChannelPair<UiPlanMessage, ExecutorPlanMessage>),
// }
//
// impl IdleMessage {
//
// }
