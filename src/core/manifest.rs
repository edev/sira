use crate::core::action::{Action, HostAction};
use crate::core::task::Task;

pub struct Manifest {
    source: Option<String>,
    name: String,
    hosts: Vec<String>,
    include: Vec<Task>,
    vars: Vec<(String, String)>,
}

#[allow(unused_variables)]
pub fn load_manifests<R: std::io::BufRead>(source: R) -> Vec<Manifest> {
    todo!()
}

impl Manifest {
    #[allow(unused_variables)]
    pub fn new(
        source: Option<String>,
        name: String,
        hosts: Vec<String>,
        include: Vec<Task>,
        vars: Vec<(String, String)>,
    ) -> Self {
        todo!()
    }

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

    /// Where this manifest came from.
    ///
    /// For instance, a manifest loaded from a file might set this to the path to the file.
    ///
    /// For manifests from other sources, e.g. directly from Rust or from network sources,
    /// there is currently no standard value to place here, because these are not intended
    /// use cases for Sira at this time.
    pub fn source(&self) -> &Option<String> {
        &self.source
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    pub fn tasks(&self) -> &[Task] {
        &self.include
    }

    pub fn vars(&self) -> &[(String, String)] {
        &self.vars
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

    /// The iterator that yields the [Action]s from `self.task`. We use these values in
    /// combination with values saved in the `TaskIter` to build [HostAction]s.
    ///
    /// If there are no tasks in the manifest, then there can be no action iterator. Thus,
    /// this must be an optional type.
    action_iter: Option<std::slice::Iter<'p, Action>>,
}

impl<'p> Iterator for TaskIter<'p> {
    type Item = HostAction<'p>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we have an `Action` iterator, and it has an `Action` for us, then we're done.
        if let Some(ref mut iter) = self.action_iter {
            if let Some(action) = iter.next() {
                return Some(HostAction::new(
                    self.host,
                    self.manifest,
                    self.task.unwrap(),
                    action,
                ));
            }
        }

        // If we have another `Task`, then save an iterator over its `Action`s and retry.
        if let Some(task) = self.task_iter.next() {
            self.action_iter = Some(task.actions().iter());
            self.task = Some(task);
            return self.next();
        }

        // If we don't have a next `Action`, and we don't have any more `Tasks` to try,
        // then we're done.
        None
    }
}
