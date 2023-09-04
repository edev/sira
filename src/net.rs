//! The public API for building network interfaces to Sira. Does not contain a network
//! implementation.
//!
//! Sira uses a network module in the controller application to connect to managed nodes. The jobs
//! of a network module are to (a) listen to instructions from [Executor], (b) establish
//! connections to managed nodes, (c) perform actions (usually calling `sira-client`) on managed
//! nodes, and (d) provide status updates to [Executor].
//!
//! # Implementing non-SSH connections
//!
//! The reference implementation (not this module) leverages SSH connections to connect to managed
//! nodes, but it's certainly possible to connect in other ways as well. The API design does make
//! some design decisions based on what works well by default with SSH and what's convenient for
//! SSH-based connections; these are documented throughout the project on a best-effort basis. In
//! general, if you have a connection method in mind that ultimately yields a shell session, you
//! can probably adapt it to Sira without too much trouble or too many surprises.
//!
//! [Executor]: crate::executor::Executor

use crate::executor::{self, ChannelPair};
use crossbeam::channel::{self, Sender};

/// Messages that a network module can send to [Executor].
///
/// [Executor]: crate::executor::Executor
#[derive(Debug)]
pub enum Message {}
