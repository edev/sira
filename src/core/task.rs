//! Types for representing task files.

use crate::core::action::Action;

/// Represents a task file; typically used in the context of a [Manifest].
///
/// This type is typically parsed from a manifest file, but it can be constructed programmatically
/// as well.
///
/// [Manifest]: crate::core::manifest::Manifest
#[derive(Clone, Debug)]
pub struct Task {
    /// The file from which this value was parsed (if any).
    source: Option<String>,

    /// Used for informational, logging, and debugging purposes.
    name: String,

    /// The user on a managed node that should run this [Task]'s [Action]s.
    ///
    /// This is **not** the user Sira will use to log into the host; `sira-client` will switch to
    /// this user to perform actions.
    user: String,

    /// Order is preserved from the source file. Actions are executed in order.
    actions: Vec<Action>,

    /// Order is preserved from the source file but is typically unimportant.
    vars: Vec<(String, String)>,
}

impl Task {
    /// Where this task came from.
    ///
    /// For instance, a task loaded from a file might set this to the path to the file.
    ///
    /// For task from other sources, e.g. directly from Rust or from network sources,
    /// there is currently no standard value to place here, because these are not intended
    /// use cases for Sira at this time.
    pub fn source(&self) -> &Option<String> {
        &self.source
    }

    /// The task's name. This has no bearing on program execution and is simply a convenience.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The user as which [Action]s should run.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The list of [Action]s that comprise this [Task], in the order specified in the file.
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// Task-level variables, which will eventually be compiled when actions are run.
    ///
    /// Variables are stored as `(name, value)` tuples.
    pub fn vars(&self) -> &[(String, String)] {
        &self.vars
    }
}
