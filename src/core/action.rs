use crate::core::{manifest::Manifest, task::Task};
pub use regex::Regex;

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

pub struct HostAction<'p> {
    host: &'p str,
    manifest: &'p Manifest,
    task: &'p Task,
    action: &'p Action,
}

impl<'p> HostAction<'p> {
    pub(in crate::core) fn new(
        host: &'p str,
        manifest: &'p Manifest,
        task: &'p Task,
        action: &'p Action,
    ) -> Self {
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
