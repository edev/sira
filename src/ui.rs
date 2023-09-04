//! The public API for building user interfaces to Sira. Does not contain a UI implementation.
//!
//! A user interface (UI) to Sira may take any form, as far as Sira is concerned. From Sira's
//! perspective, a UI consists of a [State] that defines the different states in which Sira can be,
//! e.g. idle or running a [Plan]. These states provide the communications channels to the
//! [Executor].
//!
//! A UI might also wish to clone the logger. (TODO Implement logging and a cloneable interface,
//! e.g. `Executor::logger(&self)`)
//!
//! [Executor]: crate::executor::Executor

use crate::core::plan::Plan;
use crate::executor::{self, ChannelPair};
use crossbeam::channel::{self, Sender};

/// A UI's communication channel to [Executor] when there is no currently executing [Plan].
///
/// In this state, the UI's only available interaction with Sira is to instruct [Executor] to start
/// running a [Plan]. You can do this with [IdleState::start()].
///
/// [Executor]: executor::Executor
#[derive(Debug)]
pub struct IdleState {
    /// It is a logic error to send more than one message through this channel.
    ///
    /// You should not interact with this channel directly. Instead, use [IdleState::start()].
    sender: Sender<(Plan, ChannelPair<executor::Message, Message>)>,
}

impl IdleState {
    /// Starts executing a [Plan]. Consumes self and returns a [State::Plan].
    ///
    /// This method **does not** block awaiting the completion of the [Plan]; it simply provides
    /// [Executor] the information it needs to start running the [Plan] and transitions the UI into
    /// [State::Plan].
    ///
    /// [Executor]: crate::executor::Executor
    pub fn start(self, plan: Plan) -> State {
        let (executor_tx, ui_rx) = channel::unbounded::<executor::Message>();
        let (ui_tx, executor_rx) = channel::unbounded::<Message>();

        let executor_channel_pair = ChannelPair {
            sender: executor_tx,
            receiver: executor_rx,
        };

        self.sender.send((plan, executor_channel_pair)).unwrap();

        State::Plan(PlanState {
            sender: ui_tx,
            receiver: ui_rx,
        })
    }
}

/// A UI's communication channels with [Executor] when it is executing a [Plan].
///
/// [Executor]: executor::Executor
pub type PlanState = ChannelPair<Message, executor::Message>;

/// Messages that a UI can send to [Executor].
///
/// Currently there are none, because the planned UI simply monitors a batch job.
///
/// [Executor]: executor::Executor
#[derive(Debug)]
pub enum Message {}

/// Defines the states that the UI can inhabit (from Sira's perspective).
///
/// Each state contains the communication channels that are appropriate for that state as well as
/// that state's API (e.g. [IdleState::start()]).
#[derive(Debug)]
pub enum State {
    /// The program is `idle`, meaning it is not executing a [Plan].
    ///
    /// From this state, the UI may start a [Plan] using [IdleState::start].
    Idle(IdleState),

    /// The program is currently executing a [Plan].
    ///
    /// For safety, only one [Plan] may execute at a time. Separating [State::Idle] and
    /// [State::Plan] allows Sira to enforce this through Rust's type system.
    Plan(PlanState),
}

impl State {
    /// Creates a [State::Idle] and an [executor::UiState::Idle], paired and ready to talk.
    ///
    /// This function is the intended way to instantiate both of these interconnected types.
    pub fn new() -> (Self, executor::UiState) {
        let (sender, receiver) = channel::unbounded();
        let ui_state = Self::Idle(IdleState { sender });
        let executor_state = executor::UiState::Idle(executor::IdleState::new(receiver));
        (ui_state, executor_state)
    }
}
