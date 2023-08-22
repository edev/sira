use crate::core::plan::Plan;
use crate::executor::{self, ChannelPair};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

/// A UI's communication channel to [Executor] when there is no currently executing [Plan].
///
/// [Executor]: executor::Executor
#[derive(Debug)]
pub struct IdleState {
    sender: oneshot::Sender<(Plan, ChannelPair<executor::Message, Message>)>,
}

impl IdleState {
    /// Starts executing a [Plan]. Consumes self and returns a [State::Plan].
    pub fn start(self, plan: Plan) -> State {
        let (executor_tx, ui_rx) = mpsc::unbounded_channel::<executor::Message>();
        let (ui_tx, executor_rx) = mpsc::unbounded_channel::<Message>();

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

pub type PlanState = ChannelPair<Message, executor::Message>;

/// Messages that a UI can send to [Executor].
///
/// Currently there are none, because the planned UI simply monitors a batch job.
///
/// [Executor]: executor::Executor
#[derive(Debug)]
pub enum Message {}

#[derive(Debug)]
pub enum State {
    /// The program is `idle`, meaning it is not executing a [Plan].
    ///
    /// From this state, the UI may start a [Plan] using [IdleState::start].
    Idle(IdleState),

    /// The program is currently executing a [Plan].
    ///
    /// For safety, only one [Plan] may execute at a time. Separating [State::Idle] and
    /// [State::Plan] is how we accomplish this.
    Plan(PlanState),
}

impl State {
    /// Creates a [State::Idle] and an [executor::UiState::Idle], paired and ready to talk.
    ///
    /// This function is the intended way to instantiate both of these interconnected types.
    pub fn new() -> (Self, executor::UiState) {
        let (sender, receiver) = oneshot::channel();
        let ui_state = Self::Idle(IdleState { sender });
        let executor_state = executor::UiState::Idle(executor::IdleState::new(receiver));
        (ui_state, executor_state)
    }
}
