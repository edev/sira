//! The public API for building user interfaces to Sira. Does not contain a UI implementation.
//!
//! A UI might also wish to clone the logger. (TODO Implement logging and a cloneable interface,
//! e.g. `Executor::logger(&self)`)
//!
//! [Executor]: crate::executor::Executor

use crate::core::plan::Plan;
use crate::executor;

/// The UI's channels of communication with the rest of Sira (through [Executor]).
///
/// [Executor]: crate::executor::Executor
pub type ChannelPair = executor::ChannelPair<Message, executor::Report>;

/// Messages that a UI can send to [Executor].
///
/// [Executor]: crate::executor::Executor
#[derive(Debug)]
pub enum Message {
    RunPlan(Plan),
}
