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

use crate::core::action::Action;
use crate::executor;
use std::sync::Arc;

/// The UI's channels of communication with the rest of Sira (through [Executor]).
///
/// [Executor]: crate::executor::Executor
pub type ChannelPair = executor::ChannelPair<Report, executor::NetworkControlMessage>;

/// Messages that a network module can send to [Executor].
///
/// [Executor]: crate::executor::Executor
#[derive(Debug)]
pub enum Report {
    Connecting(String),

    Connected(String),

    FailedToConnect {
        host: String,
        error: String,
    },

    Disconnected {
        host: String,

        /// If the disconnect was the result of some kind of error, any available information will
        /// be returned here. If the disconnect does not indicate an error, then this field will be
        /// `None`.
        error: Option<String>,
    },

    // Note that we can move to using lots of `Arc`s, here and elsewhere, if we want, to reduce
    // allocations and copies. They would have to be just about everywhere, though, given the
    // data we send across messages.
    RunningAction {
        host: String,
        // Add `plan: String,` here if we give plans names, which we should if we let them queue.
        manifest_source: Option<String>,
        manifest_name: String,
        task_source: Option<String>,
        task_name: String,
        action: Arc<Action>,
    },
}
