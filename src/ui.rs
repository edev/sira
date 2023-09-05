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

/// Messages that a UI can send to [Executor].
///
/// Currently there are none, because the planned UI simply monitors a batch job.
///
/// [Executor]: executor::Executor
#[derive(Debug)]
pub enum Message {}
