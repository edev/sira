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

use crate::core::action::Action;
use crate::executor;
#[cfg(doc)]
use crate::executor::Executor;
use std::fmt;
use std::sync::Arc;

/// The UI's channels of communication with the rest of Sira (through [Executor]).
pub type ChannelPair = executor::ChannelPair<Report, executor::NetworkControlMessage>;

/// Messages that a network module can send to [Executor].
#[derive(Debug)]
pub enum Report {
    /// The network module is about to try connecting to the specified host.
    Connecting(String),

    /// The network module has successfully connected to the specified host.
    Connected(String),

    /// The network module couldn't reach the host.
    ///
    /// When [Executor] receives this message, it is free to choose any response strategy, such as
    /// retrying or aborting.
    FailedToConnect { host: String, error: String },

    /// The network module had a connection to the host, but the connection is now closed.
    Disconnected {
        host: String,

        /// If the disconnect was the result of some kind of error, any available information will
        /// be returned here. If the disconnect does not indicate an error, then this field will be
        /// `None`.
        error: Option<String>,
    },

    /// The network module is sending the compiled [Action] to the host for execution.
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

    /// The specified [Action] is has finished running.
    ///
    /// This message does not imply success; for the outcome, see the [result] field.
    ///
    /// [result]: Result::ActionResult::result]
    // TODO Rename ActionResult, e.g. to ActionReport, to avoid confusion with Result types.
    ActionResult {
        host: String,
        // Add `plan: String,` here if we give plans names, which we should if we let them queue.
        manifest_source: Option<String>,
        manifest_name: String,
        task_source: Option<String>,
        task_name: String,
        action: Arc<Action>,
        result: Result<(), ()>,
    },
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Report::*;
        match self {
            // The network module is about to try connecting to the specified host.
            Connecting(host) => write!(f, "Connecting to {}", host),

            // The network module has successfully connected to the specified host.
            Connected(host) => write!(f, "Connected to {}", host),

            // The network module couldn't reach the host.
            FailedToConnect { host, error } => write!(f, "Couldn't connect to {}: {}", host, error),

            // The network module had a connection to the host, but the connection is now closed.
            Disconnected { host, error: None } => write!(f, "Disconnected from {}", host),
            Disconnected {
                host,
                error: Some(error),
            } => write!(f, "Disconnected from {} due to error: {}", host, error),

            // The network module is sending the compiled [Action] to the host for execution.
            RunningAction {
                host,
                manifest_source,
                manifest_name,
                task_source,
                task_name,
                action,
            } => {
                // TODO Replace this with a YAML serialization when possible.
                write!(
                    f,
                    "Running action on {}:\n\
                    \tManifest: {} ({})\n\
                    \tTask: {} ({})\n\
                    \tAction: {:?}",
                    host,
                    manifest_name,
                    manifest_source.as_deref().unwrap_or("Unknown source"),
                    task_name,
                    task_source.as_deref().unwrap_or("Unknown source"),
                    action,
                )
            }

            // The specified [Action] is has finished running.
            ActionResult {
                host,
                manifest_source,
                manifest_name,
                task_source,
                task_name,
                action,
                result,
            } => {
                write!(
                    f,
                    "Action complete on {}:\n\
                    \tManifest: {} ({})\n\
                    \tTask: {} ({})\n\
                    \tAction: {:?}\n\
                    \tResult: {:?}",
                    host,
                    manifest_name,
                    manifest_source.as_deref().unwrap_or("Unknown source"),
                    task_name,
                    task_source.as_deref().unwrap_or("Unknown source"),
                    action,
                    result,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod report {
        use super::*;

        mod display {
            use super::*;
            use Report::*;

            #[test]
            fn connecting() {
                assert_eq!(
                    "Connecting to hostname",
                    Connecting("hostname".to_string()).to_string(),
                );
            }

            #[test]
            fn connected() {
                assert_eq!(
                    "Connected to hostname",
                    Connected("hostname".to_string()).to_string(),
                );
            }

            #[test]
            fn failed_to_connect() {
                assert_eq!(
                    "Couldn't connect to hostname: error",
                    FailedToConnect {
                        host: "hostname".to_string(),
                        error: "error".to_string(),
                    }
                    .to_string(),
                );
            }

            #[test]
            fn disconnected() {
                assert_eq!(
                    "Disconnected from hostname",
                    Disconnected {
                        host: "hostname".to_string(),
                        error: None,
                    }
                    .to_string(),
                );

                assert_eq!(
                    "Disconnected from hostname due to error: error",
                    Disconnected {
                        host: "hostname".to_string(),
                        error: Some("error".to_string()),
                    }
                    .to_string(),
                );
            }

            #[test]
            fn running_action() {
                assert_eq!(
                    "Running action on host:\n\
                    \tManifest: mname (mani.fest)\n\
                    \tTask: tname (ta.sk)\n\
                    \tAction: Shell { commands: [\"pwd\"] }",
                    RunningAction {
                        host: "host".to_string(),
                        manifest_source: Some("mani.fest".to_string()),
                        manifest_name: "mname".to_string(),
                        task_source: Some("ta.sk".to_string()),
                        task_name: "tname".to_string(),
                        action: Arc::new(Action::Shell {
                            commands: vec!["pwd".to_string()],
                        }),
                    }
                    .to_string(),
                );

                assert_eq!(
                    "Running action on host:\n\
                    \tManifest: mname (Unknown source)\n\
                    \tTask: tname (Unknown source)\n\
                    \tAction: Shell { commands: [\"pwd\"] }",
                    RunningAction {
                        host: "host".to_string(),
                        manifest_source: None,
                        manifest_name: "mname".to_string(),
                        task_source: None,
                        task_name: "tname".to_string(),
                        action: Arc::new(Action::Shell {
                            commands: vec!["pwd".to_string()],
                        }),
                    }
                    .to_string(),
                );
            }

            #[test]
            fn action_result() {
                assert_eq!(
                    "Action complete on host:\n\
                    \tManifest: mname (mani.fest)\n\
                    \tTask: tname (ta.sk)\n\
                    \tAction: Shell { commands: [\"pwd\"] }\n\
                    \tResult: Ok(())",
                    ActionResult {
                        host: "host".to_string(),
                        manifest_source: Some("mani.fest".to_string()),
                        manifest_name: "mname".to_string(),
                        task_source: Some("ta.sk".to_string()),
                        task_name: "tname".to_string(),
                        action: Arc::new(Action::Shell {
                            commands: vec!["pwd".to_string()],
                        }),
                        result: Ok(()),
                    }
                    .to_string(),
                );

                assert_eq!(
                    "Action complete on host:\n\
                    \tManifest: mname (Unknown source)\n\
                    \tTask: tname (Unknown source)\n\
                    \tAction: Shell { commands: [\"pwd\"] }\n\
                    \tResult: Ok(())",
                    ActionResult {
                        host: "host".to_string(),
                        manifest_source: None,
                        manifest_name: "mname".to_string(),
                        task_source: None,
                        task_name: "tname".to_string(),
                        action: Arc::new(Action::Shell {
                            commands: vec!["pwd".to_string()],
                        }),
                        result: Ok(()),
                    }
                    .to_string(),
                );
            }
        }
    }
}
