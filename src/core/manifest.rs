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
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Debug)]
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
#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::super::fixtures::plan;
    use super::*;

    mod manifest {
        use super::*;

        // These tests are parallel to those for tasks_for.
        mod tasks_for {
            use super::*;

            #[test]
            fn works() {
                let (_, manifest, _, _) = plan();

                let maybe_task_iter: Option<TaskIter> = manifest.tasks_for(&manifest.hosts[0]);
                assert!(
                    maybe_task_iter.is_some(),
                    "Manifest::tasks_for should return a Some value when given a valid host"
                );
                let task_iter = maybe_task_iter.unwrap();

                assert_eq!(&manifest.hosts[0], &task_iter.host);

                assert_eq!(&manifest, task_iter.manifest);

                assert!(task_iter.task.is_none());

                assert_eq!(
                    manifest.include.iter().collect::<Vec<&Task>>(),
                    task_iter.manifest.include.iter().collect::<Vec<&Task>>(),
                );

                assert!(task_iter.action_iter.is_none());
            }

            #[test]
            fn works_for_nth_host() {
                let (_, mut manifest, _, _) = plan();

                // Push a few extra hosts; we'll query one of the middle entries in a moment.
                manifest.hosts.push("garden".into());
                manifest.hosts.push("gardenia".into());
                manifest.hosts.push("chrysanthemum".into());

                let maybe_task_iter: Option<TaskIter> = manifest.tasks_for("gardenia");
                assert!(
                    maybe_task_iter.is_some(),
                    "Manifest::tasks_for should return a Some value when given a valid host"
                );
                let task_iter = maybe_task_iter.unwrap();

                assert_eq!(&"gardenia", &task_iter.host);

                assert_eq!(&manifest, task_iter.manifest);

                assert!(task_iter.task.is_none());

                assert_eq!(
                    manifest.include.iter().collect::<Vec<&Task>>(),
                    task_iter.manifest.include.iter().collect::<Vec<&Task>>(),
                );

                assert!(task_iter.action_iter.is_none());
            }

            #[test]
            fn returns_none_if_host_not_found() {
                let (_, manifest, _, _) = plan();

                let maybe_task_iter: Option<TaskIter> = manifest.tasks_for("gardenia");
                assert!(maybe_task_iter.is_none());
            }
        }

        // These tests are parallel to those for tasks_for.
        mod into_tasks_for {
            use super::*;

            #[test]
            fn works() {
                let (_, manifest, _, _) = plan();

                let maybe_task_into_iter: Option<TaskIntoIter> =
                    manifest.clone().into_tasks_for(&manifest.hosts[0]);
                assert!(
                    maybe_task_into_iter.is_some(),
                    "Manifest::tasks_for should return a Some value when given a valid host"
                );
                let task_into_iter = maybe_task_into_iter.unwrap();

                assert_eq!(&manifest.hosts[0], &task_into_iter.host);

                assert_eq!(&manifest, &task_into_iter.manifest);

                assert!(task_into_iter.task.is_none());

                assert_eq!(
                    manifest.include.iter().collect::<Vec<&Task>>(),
                    task_into_iter
                        .manifest
                        .include
                        .iter()
                        .collect::<Vec<&Task>>(),
                );

                assert!(task_into_iter.action_iter.is_none());
            }

            #[test]
            fn works_for_nth_host() {
                let (_, mut manifest, _, _) = plan();

                // Push a few extra hosts; we'll query one of the middle entries in a moment.
                manifest.hosts.push("garden".into());
                manifest.hosts.push("gardenia".into());
                manifest.hosts.push("chrysanthemum".into());

                let maybe_task_into_iter: Option<TaskIntoIter> =
                    manifest.clone().into_tasks_for("gardenia");
                assert!(
                    maybe_task_into_iter.is_some(),
                    "Manifest::tasks_for should return a Some value when given a valid host"
                );
                let task_into_iter = maybe_task_into_iter.unwrap();

                assert_eq!(&"gardenia", &task_into_iter.host);

                assert_eq!(&manifest, &task_into_iter.manifest);

                assert!(task_into_iter.task.is_none());

                assert_eq!(
                    manifest.include.iter().collect::<Vec<&Task>>(),
                    task_into_iter
                        .manifest
                        .include
                        .iter()
                        .collect::<Vec<&Task>>(),
                );

                assert!(task_into_iter.action_iter.is_none());
            }

            #[test]
            fn returns_none_if_host_not_found() {
                let (_, manifest, _, _) = plan();

                let maybe_task_into_iter: Option<TaskIntoIter> =
                    manifest.into_tasks_for("gardenia");

                assert!(maybe_task_into_iter.is_none());
            }
        }
    }

    // We have two iterators that do the exact same things, one for owned values and one for
    // references. Both produce [HostAction] values, and both follow parallel logic. Therefore, it
    // makes the most sense to test them together, as we do below.
    //
    // Rather then picking apart the intricacies of every possible behavior for this multi-level
    // iteration, we simply provide a single, complex test that puts it through its paces fairly
    // thoroughly and test a couple of edge cases separately. It's possible to spend much more time
    // doing much more thorough testing, but the algorithm is simple and mostly relies on proven
    // standard library code, so these tests are assumed sufficient until proven otherwise.
    mod iterators {
        use super::*;

        #[test]
        fn returns_all_actions_for_all_manifests_and_tasks() {
            // Actions for Task 1 (below).
            let task_1_actions = vec![
                Action::Shell {
                    commands: vec!["echo hi".into(), "pwd".into()],
                },
                Action::LineInFile {
                    after: "localhost".into(),
                    insert: vec!["192.168.1.93 zen3".into()],
                    path: "/etc/hosts".into(),
                },
                Action::Upload {
                    from: "from".into(),
                    to: "to".into(),
                },
                Action::Download {
                    from: "from".into(),
                    to: "to".into(),
                },
            ];

            // Task 2 has no actions.

            // Actions for Task 3 (below).
            let task_3_actions = vec![Action::Shell {
                commands: vec!["echo bye".into(), "logout".into()],
            }];

            let tasks = vec![
                // A normal, routine task.
                Task {
                    source: None,
                    name: "Task 1".into(),
                    user: "george".into(),
                    actions: task_1_actions.clone(),
                    vars: vec![],
                },
                // A corner case: a task that's empty.
                Task {
                    source: None,
                    name: "Task 2".into(),
                    user: "george".into(),
                    actions: vec![],
                    vars: vec![],
                },
                // Another routine task afterward.
                Task {
                    source: None,
                    name: "Task 3".into(),
                    user: "george".into(),
                    actions: task_3_actions.clone(),
                    vars: vec![],
                },
            ];

            let manifest = Manifest {
                source: None,
                name: "API test".into(),
                hosts: vec!["api_test".into()],
                include: tasks,
                vars: vec![],
            };

            let task_1_host_actions = task_1_actions.into_iter().map(|action| {
                Arc::new(HostAction::new(
                    "api_test",
                    &manifest,
                    &manifest.include[0],
                    &action,
                ))
            });

            let task_3_host_actions = task_3_actions.into_iter().map(|action| {
                Arc::new(HostAction::new(
                    "api_test",
                    &manifest,
                    &manifest.include[2],
                    &action,
                ))
            });

            let expected_host_actions: Vec<Arc<HostAction>> =
                task_1_host_actions.chain(task_3_host_actions).collect();

            assert_eq!(
                expected_host_actions,
                manifest
                    .tasks_for("api_test")
                    .unwrap()
                    .collect::<Vec<Arc<HostAction>>>(),
            );

            assert_eq!(
                expected_host_actions,
                manifest
                    .into_tasks_for("api_test")
                    .unwrap()
                    .collect::<Vec<Arc<HostAction>>>(),
            );
        }

        #[test]
        fn returns_none_if_no_tasks() {
            let manifest = Manifest {
                source: None,
                name: "API test".into(),
                hosts: vec!["api_test".into()],
                include: vec![],
                vars: vec![],
            };

            let mut task_iter = manifest.tasks_for("api_test").unwrap();
            assert!(task_iter.next().is_none());

            let mut task_into_iter = manifest.into_tasks_for("api_test").unwrap();
            assert!(task_into_iter.next().is_none());
        }

        #[test]
        fn returns_none_if_no_actions_in_last_task() {
            let task = Task {
                source: None,
                name: "API test".into(),
                user: "george".into(),
                actions: vec![],
                vars: vec![],
            };

            let manifest = Manifest {
                source: None,
                name: "API test".into(),
                hosts: vec!["api_test".into()],
                include: vec![task],
                vars: vec![],
            };

            let mut task_iter = manifest.tasks_for("api_test").unwrap();
            assert!(task_iter.next().is_none());

            let mut task_into_iter = manifest.into_tasks_for("api_test").unwrap();
            assert!(task_into_iter.next().is_none());
        }
    }
}
