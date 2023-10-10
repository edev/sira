//! The public API for building user interfaces to Sira. Does not contain a UI implementation.
//!
//! A UI might also wish to clone [logger::Log].

use crate::core::plan::Plan;
use crate::executor;
#[cfg(doc)]
use crate::executor::Executor;
#[cfg(doc)]
use crate::logger;

/// The UI's channels of communication with the rest of Sira (through [Executor]).
pub type ChannelPair = executor::ChannelPair<Message, executor::Report>;

/// Messages that a UI can send to [Executor].
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Asks [Executor] to execute a [Plan].
    ///
    /// If it is already running a plan, this one will be added to the queue. Otherwise, it will
    /// execute immediately.
    RunPlan(Plan),
}
