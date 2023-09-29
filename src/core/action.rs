//! Types for representing individual actions.

#[cfg(doc)]
use crate::core::plan::Plan;
use crate::core::{manifest::Manifest, task::Task};
#[cfg(doc)]
use crate::executor::Executor;
#[cfg(doc)]
use std::sync::Arc;

/// The types of actions that Sira can perform on a client.
// TODO Flesh out Actions. The current states are intentionally basic sketches.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Shell {
        commands: Vec<String>,
    },

    // I need to visit Ansible's docs to fill out this struct. A lot more needs to go here, I
    // strongly suspect.
    LineInFile {
        after: String,
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
#[derive(Clone, Debug, PartialEq)]
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
        assert!(
            manifest.hosts.iter().any(|hst| hst == host),
            "Cannot create HostAction for manifest \"{}\" and host \"{}\" because the manifest does not \
            include this host:\n\
            {:?}",
            manifest.name,
            host,
            manifest,
        );
        assert!(
            manifest.include.iter().any(|tsk| tsk == task),
            "Cannot create HostAction for manifest \"{}\" and task \"{}\" because the manifest does not \
            include this task:\n\
            {:?}\n\
            {:?}",
            manifest.name,
            task.name,
            manifest,
            task,
        );
        assert!(
            task.actions.iter().any(|act| act == action),
            "Cannot create HostAction for manifest \"{}\" and task \"{}\" because the task does not \
            include this action:\n\
            {:?}\n\
            Action::{:?}",
            manifest.name,
            task.name,
            task,
            action,
        );

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

#[cfg(test)]
mod tests {
    use super::super::fixtures::plan;
    use super::*;

    mod host_action {
        use super::*;

        mod new {
            use super::*;

            #[test]
            fn works() {
                let (_, manifest, task, action) = plan();
                let host_action = HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
                assert_eq!(manifest.hosts[0], host_action.host);
                assert_eq!(manifest, host_action.manifest);
                assert_eq!(task, host_action.task);
                assert_eq!(action, host_action.action);
            }

            #[test]
            #[should_panic(expected = "manifest does not include this host")]
            fn requires_manifest_to_include_host() {
                let (_, manifest, task, action) = plan();
                let host = "host-not-included";
                HostAction::new(host, &manifest, &task, &action);
            }

            #[test]
            #[should_panic(expected = "manifest does not include this task")]
            fn requires_manifest_to_include_task() {
                let (_, manifest, _, action) = plan();
                let task = Task {
                    source: None,
                    name: "task-not-included".into(),
                    user: "zane".into(),
                    actions: vec![],
                    vars: vec![],
                };
                HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            }

            #[test]
            #[should_panic(expected = "task does not include this action")]
            fn requires_task_to_include_action() {
                let (_, manifest, task, _) = plan();
                let action = Action::Shell { commands: vec![] };
                HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            }
        }
    }
}
