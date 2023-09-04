//! Types for representing individual actions.

use crate::core::{manifest::Manifest, task::Task};
use regex::Regex;

/// The types of actions that Sira can perform on a client.
#[derive(Clone, Debug)]
pub enum Action {
    Shell {
        commands: Vec<String>,
    },

    // I need to visit Ansible's docs to fill out this struct. A lot more needs to go here, I
    // strongly suspect.
    LineInFile {
        after: Regex,
        insert: Vec<String>,
        path: String,
    },

    // I need to add more fields, like user, group, and permissions.
    Upload {
        from: String,
        to: String,
    },

    // I need to add more fields, like user, group, and permissions.
    Download {
        from: String,
        to: String,
    },
}

/// An [Action] in the context of a single [Manifest], [Task], and host.
///
/// A [HostAction] is typically produced by running a [Plan]. The [HostAction] contains all the
/// information needed to run an [Action] on a given host as well as information about where the
/// [Action] was specified, for informational, logging, and debugging purposes.
///
/// # Optimization
///
/// Note that this type has a lot of room for optimization. Speed shouldn't be a factor at the
/// scale Sira is designed for, but it's worth noting briefly that this type makes an awful lot of
/// copies. [HostAction] values get passed from [Executor] to all other parts of the program, so
/// references are not a great fit.
///
/// Optimizing this type is a good candidate for future work. An easy approach would be to pare
/// down unneeded fields before making copies. For instance, with the [Manifest], you could strip
/// away the [Task]s, since this type shouldn't need them. There are more aggressive options, too.
///
/// A safer option might be to move aggressively into using [Arc]s for storing [Manifest]s,
/// [Task]s, and [Action]s both here and in their normal [Plan]-[Manifest]-[Task]-[Action]
/// hierarchy.
///
/// [Arc]: std::sync::Arc
/// [Executor]: crate::executor::Executor
/// [Plan]: crate::core::plan::Plan
#[derive(Clone, Debug)]
pub struct HostAction {
    /// The host on which this [Action] should run.
    host: String,

    /// The [Manifest] that listed the [Task] containing this [Action].
    manifest: Manifest,

    /// The [Task] that listed this [Action].
    task: Task,

    /// The [Action] to be executed on the host.
    action: Action,
}

impl HostAction {
    /// Creates a new [HostAction].
    ///
    /// # Panics
    ///
    /// Panics if the values provided are not sane. For instance, `manifest` must specify that
    /// `task` run on `host`, and `task` must specify `action`. Violating these sanity checks would
    /// result in unwanted (though well-defined) behavior and is clearly a bug in the calling code.
    pub(in crate::core) fn new<'plan>(
        host: &'plan str,
        manifest: &'plan Manifest,
        task: &'plan Task,
        action: &'plan Action,
    ) -> Self {
        // TODO Perform sanity checks.

        HostAction {
            host: host.to_string(),
            manifest: manifest.clone(),
            task: task.clone(),
            action: action.clone(),
        }
    }

    /// The target host name.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The manifest that caused this [Action] to run.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The [Task] that contains this [Action].
    pub fn task(&self) -> &Task {
        &self.task
    }

    /// The original [Action] from the [Task].
    pub fn action(&self) -> &Action {
        &self.action
    }
}

impl HostAction {
    /// Prepares an [Action] to be sent to a host for execution, e.g. performing variable
    /// substitution.
    #[allow(dead_code)]
    pub(in crate::core) fn compile(&self) -> Action {
        todo!()
    }
}
