//! Types for representing manifest files.
use crate::core::action::{Action, HostAction};
#[cfg(doc)]
use crate::core::plan::Plan;
use crate::core::task::Task;
use std::sync::Arc;

/// Represents a manifest file; typically used in the context of a [Plan].
///
/// This type is typically parsed from a manifest file, but it can be constructed programmatically
/// as well.
#[derive(Clone, Debug)]
pub struct Manifest {
    /// Where this manifest came from.
    ///
    /// For instance, a manifest loaded from a file should set this to the path to the file.
    ///
    /// For manifests from other sources, e.g. directly from Rust or from network sources,
    /// there is currently no standard value to place here, because these are not intended
    /// use cases for Sira at this time.
    pub source: Option<String>,

    /// The [Manifest]'s name. Used for informational, logging, and debugging purposes.
    pub name: String,

    /// The list of hosts on which this manifest will run.
    ///
    /// Order is perserved from the source file but is typically unimportant.
    pub hosts: Vec<String>,

    /// [Task]s (typically loaded from task files) that comprise this manifest.
    ///
    /// Order is preserved from the source file. Tasks are executed in order.
    pub include: Vec<Task>,

    /// [Manifest]-level variables, which will eventually be compiled when actions are run.
    ///
    /// Variables are stored as `(name, value)` tuples.
    ///
    /// Order is preserved from the source file but is typically unimportant.
    pub vars: Vec<(String, String)>,
}

/// Loads [Manifest] values from a manifest file.
#[allow(unused_variables)]
pub fn load_manifests<R: std::io::BufRead>(source: R) -> Vec<Manifest> {
    todo!()
}

impl Manifest {
    /// Returns a [TaskIter] over tasks in this manifest, or [None] if `host` doesn't
    /// match.
    pub(in crate::core) fn tasks_for<'p>(&'p self, host: &'p str) -> Option<TaskIter<'p>> {
        if self.hosts.iter().all(|h| host != h) {
            return None;
        }

        Some(TaskIter {
            host,
            manifest: self,
            task: None,
            task_iter: self.include.iter(),
            action_iter: None,
        })
    }

    /// Owned version of [Self::tasks_for].
    pub(in crate::core) fn into_tasks_for(self, host: impl Into<String>) -> Option<TaskIntoIter> {
        let host = host.into();

        if self.hosts.iter().all(|h| &host != h) {
            return None;
        }

        let task_iter = self.include.clone().into_iter();
        Some(TaskIntoIter {
            host,
            manifest: self,
            task: None,
            task_iter,
            action_iter: None,
        })
    }
}

/// Iterates over [Task]s in a [Manifest].
///
/// Returns [HostAction] values representing a given [Action] in the context of a host and
/// [Manifest].
pub(in crate::core) struct TaskIter<'p> {
    /// The host on which these tasks will run.
    ///
    /// Passed through to [HostAction].
    host: &'p str,

    /// The [Manifest] that included this [Task].
    ///
    /// Passed through to [HostAction].
    manifest: &'p Manifest,

    /// The [Task] from which [Action]s are currently being read.
    ///
    /// Passed through to [HostAction].
    task: Option<&'p Task>,

    /// The iterator that yields the [Task]s that TaskIter walks.
    task_iter: std::slice::Iter<'p, Task>,

    /// The iterator that yields the [Action]s from [Self::task]. We use these values in
    /// combination with values saved in the [TaskIter] to build [HostAction]s.
    ///
    /// If there are no tasks in the manifest, then there can be no action iterator. Thus,
    /// this must be an optional type.
    action_iter: Option<std::slice::Iter<'p, Action>>,
}

impl<'p> Iterator for TaskIter<'p> {
    type Item = Arc<HostAction>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we have an `Action` iterator, and it has an `Action` for us, then we're done.
        if let Some(ref mut iter) = self.action_iter {
            if let Some(action) = iter.next() {
                return Some(Arc::new(HostAction::new(
                    self.host,
                    self.manifest,
                    self.task.unwrap(),
                    action,
                )));
            }
        }

        // If we have another `Task`, then save an iterator over its `Action`s and retry.
        if let Some(task) = self.task_iter.next() {
            self.action_iter = Some(task.actions.iter());
            self.task = Some(task);
            return self.next();
        }

        // If we don't have a next `Action`, and we don't have any more `Tasks` to try,
        // then we're done.
        None
    }
}

/// Owned version of [TaskIter].
pub(in crate::core) struct TaskIntoIter {
    /// The host on which these tasks will run.
    ///
    /// Passed through to [HostAction].
    host: String,

    /// The [Manifest] that included this [Task].
    ///
    /// Passed through to [HostAction].
    manifest: Manifest,

    /// The [Task] from which [Action]s are currently being read.
    ///
    /// Passed through to [HostAction].
    task: Option<Task>,

    /// The iterator that yields the [Task]s that TaskIter walks.
    task_iter: std::vec::IntoIter<Task>,

    /// The iterator that yields [Action]s for the current [Task]. We use these values in
    /// combination with values saved in the [TaskIntoIter] to build [HostAction]s.
    ///
    /// If there are no tasks in the manifest, then there can be no action iterator. Thus,
    /// this must be an optional type.
    ///
    /// Unlike [TaskIter::action_iter], this field owns its [Action]s and moves them out as it
    /// yields them. It does not reference [Self::task], though that field holds copies of the same
    /// [Action]s.
    action_iter: Option<std::vec::IntoIter<Action>>,
}

impl Iterator for TaskIntoIter {
    type Item = Arc<HostAction>;

    fn next(&mut self) -> Option<Self::Item> {
        // Modeled after [TaskIter].

        if let Some(ref mut iter) = self.action_iter {
            if let Some(action) = iter.next() {
                // We own all of the values we need to return, but we need them more than once, and
                // the return value needs to own its memory. Therefore, we use the same initializer
                // as [TaskIter], which takes references and clones the values.
                return Some(Arc::new(HostAction::new(
                    &self.host,
                    &self.manifest,
                    self.task.as_ref().unwrap(),
                    &action,
                )));
            }
        }

        if let Some(task) = self.task_iter.next() {
            // We need to clone so we don't partially move out of `task`. We do, unfortunately,
            // need multiple copies of that memory.
            self.action_iter = Some(task.actions.clone().into_iter());
            self.task = Some(task);
            return self.next();
        }

        None
    }
}
