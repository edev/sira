//! Types for representing individual actions.

use crate::core::{manifest::Manifest, task::Task};
use regex::Regex;

/// The types of actions that Sira can perform on a client.
#[derive(Debug)]
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
/// [Plan]: crate::core::plan::Plan
pub struct HostAction<'p> {
    /// The host on which this [Action] should run.
    host: &'p str,

    /// The [Manifest] that listed the [Task] containing this [Action].
    manifest: &'p Manifest,

    /// The [Task] that listed this [Action].
    task: &'p Task,

    /// The [Action] to be executed on the host.
    action: &'p Action,
}

impl<'p> HostAction<'p> {
    /// Creates a new [HostAction].
    ///
    /// # Panics
    ///
    /// Panics if the values provided are not sane. For instance, `manifest` must specify that
    /// `task` run on `host`, and `task` must specify `action`. Violating these sanity checks would
    /// result in unwanted (though well-defined) behavior and is clearly a bug in the calling code.
    pub(in crate::core) fn new(
        host: &'p str,
        manifest: &'p Manifest,
        task: &'p Task,
        action: &'p Action,
    ) -> Self {
        // TODO Perform sanity checks.

        HostAction {
            host,
            manifest,
            task,
            action,
        }
    }

    /// The target host name.
    pub fn host(&self) -> &str {
        self.host
    }

    /// The manifest that caused this [Action] to run.
    pub fn manifest(&self) -> &Manifest {
        self.manifest
    }

    /// The [Task] that contains this [Action].
    pub fn task(&self) -> &Task {
        self.task
    }

    /// The original [Action] from the [Task].
    pub fn action(&self) -> &Action {
        self.action
    }
}

impl<'m> HostAction<'m> {
    /// Prepares an [Action] to be sent to a host for execution, e.g. performing variable
    /// substitution.
    #[allow(dead_code)]
    pub(in crate::core) fn compile(&self) -> Action {
        todo!()
    }
}
