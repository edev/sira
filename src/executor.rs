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

use crate::core::plan::Plan;
use crate::ui;
use tokio;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

#[allow(dead_code)]
pub struct Executor {
    ui: UiState,
    logger: ChannelPair<(), ()>,
    network: ChannelPair<(), ()>,
}

#[derive(Debug)]
pub struct ChannelPair<S, R> {
    pub sender: UnboundedSender<S>,
    pub receiver: UnboundedReceiver<R>,
}

impl Executor {
    pub fn new(ui: UiState, logger: ChannelPair<(), ()>, network: ChannelPair<(), ()>) -> Self {
        Executor {
            ui,
            logger,
            network,
        }
    }

    // async fn ui_listener(receiver: Receiver<()>, logger: Sender<()>, network: ()) -> Result<(), ()> {
    //     todo!()
    // }
}

#[derive(Debug)]
pub enum Message {
    /// Sent when [Executor] is returning to the idle state (no longer executing a plan).
    ///
    /// When the UI receives this, it should switch its [ui::State] to [ui::State::Idle].
    Idle(ui::IdleState),
}

#[derive(Debug)]
pub enum UiState {
    Idle(IdleState),
    Plan(PlanState),
}

#[derive(Debug)]
pub struct IdleState {
    receiver: oneshot::Receiver<(Plan, ChannelPair<Message, ui::Message>)>,
}

impl IdleState {
    /// Waits until a UI calls [ui::IdleState::start], then returns a [UiState::Plan].
    pub async fn wait_to_start(self) -> UiState {
        let _response = tokio::spawn(async move { self.receiver.blocking_recv() }).await;

        todo!()
    }
}

pub type PlanState = ChannelPair<Message, ui::Message>;
