//! At this stage, this file contains a very rough initial sketch of the program's organization.

#![allow(dead_code)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

// Manifest files and Task files. These are just the names for the file types; you can think of
// them in whatever terms work for you, e.g. playbooks, roles and profiles, etc.

pub mod core {
    pub mod plan {
        use crate::core::action::{Action, HostAction};
        use crate::core::manifest::{Manifest, TaskIter};
        use crate::core::task::{HostTask, Task};
        use std::path::Path;

        /// A plan of action for executing a given list of manifests.
        ///
        /// This struct constitutes the public interface that executors use to interact with
        /// [Manifest]s, [Task]s, and [Action]s on the controller.
        pub struct Plan {
            /// The official, ordered list of manifests that comprise the plan.
            ///
            /// Everything else can be computed from these manifests.
            manifests: Vec<Manifest>,
        }

        impl Plan {
            pub fn new() -> Self {
                todo!()
            }

            /// Add a [Manifest] to the plan.
            ///
            /// This manifest will run after all previously added manifests.
            pub fn add_manifest(&mut self, manifest: Manifest) {}

            /// Add a list of [Manifest]s to the plan.
            ///
            /// These manifests will run in the order specified, but only after all previously
            /// added manifests.
            pub fn add_manifests(&mut self, manifests: Vec<Manifest>) {}

            /// Returns a list of hosts involved in this `Plan`.
            ///
            /// In order to prevent TOCTOU bugs in executors, this method deliberately returns
            /// an immutable reference into the `Plan`. This prevents executors from adding new
            /// manifests or otherwise modifying the plan after retrieving the list of hosts. If
            /// you choose to clone the returned list of hosts and drop the reference, you are
            /// taking responsibility for ensuring that you don't introduce any TOCTOU bugs.
            pub fn hosts(&self) -> &[String] {
                todo!()
            }

            /// Returns an execution plan for the specified host.
            ///
            /// Returns [None] if `host` was not in the plan's list of hosts.
            pub fn plan_for(&self, host: &str) -> Option<HostPlan> {
                todo!()
            }
        }

        pub struct HostPlan<'p> {
            /// The host on which this plan will run.
            host: &'p str,

            /// The [Plan] to run on the host.
            plan: &'p Plan,
        }

        impl<'p> HostPlan<'p> {
            pub fn iter(&self) -> HostPlanIter {
                HostPlanIter {
                    host: self.host,
                    manifests: self.plan.manifests.iter(),

                    // `current_iter` must be `None`, otherwise we inadvertently bypass the logic
                    // in the `Iterator::next` method that skips manifests that don't shouldn't run
                    // on `host`.
                    current_iter: None,
                }
            }
        }

        /// An iterator that yields actions to take on a specific host, in order.
        pub struct HostPlanIter<'p> {
            /// The host on which the plan is intended to run.
            host: &'p str,

            /// An iterator over the manifests in this plan.
            manifests: std::slice::Iter<'p, Manifest>,

            /// The current task iterator, which yields [HostAction] values.
            ///
            /// If there are no manifests in the plan, then there can be no current iterator. Thus,
            /// this must be an optional type.
            current_iter: Option<TaskIter<'p>>,
        }

        impl<'p> Iterator for HostPlanIter<'p> {
            type Item = HostAction<'p>;

            fn next(&mut self) -> Option<Self::Item> {
                // A `TaskIter` knows how to walk a list of tasks and return a single
                // `HostAction`. Our job here is to walk the list of manifests, in order,
                // generating a new `Taskiter` from the next `Manifest` when the previous one is
                // done. When we're out of manifests, we're done.

                // If we have an iterator and it has a value for us, we're done.
                if let Some(ref mut iter) = self.current_iter {
                    if let Some(next) = iter.next() {
                        return Some(next);
                    }
                }

                // If we have another manifest to try, save its iterator to `current_iter` and
                // try again. Skip any manifests that that shouldn't run on `host`.
                if let Some(next_manifest) = self.manifests.next() {
                    self.current_iter = next_manifest.tasks_for(self.host);
                    return self.next();
                }

                // If we don't have a `TaskIter` ready to yield a value, and we don't have another
                // `Manifest` to try, then we're done.
                None
            }
        }

        /// This function is meant as the easy, default entry point for executors.
        ///
        /// This function doesn't actually know how to parse; it simply calls
        /// core::manifest::load_manifests for each file.
        ///
        /// The return type should be a Result<Self, Error>, but I haven't defined the error yet.
        pub fn from_manifest_files(files: &[impl AsRef<Path>]) -> Plan {
            todo!()
        }
    }

    pub mod manifest {
        use crate::core::action::{Action, HostAction};
        use crate::core::task::Task;
        pub struct Manifest {
            source: Option<String>,
            name: String,
            hosts: Vec<String>,
            include: Vec<Task>,
            vars: Vec<(String, String)>,
        }

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
        /// Some fields of this iterator are for tracking interation progress. Others are saving
        /// information we'll need to pass on to lower-level iterators so that the [HostAction] has
        /// all the information it needs. This distinction is documented in each field's comments.
        ///
        /// # Returns
        ///
        /// [HostAction] values representing a given [Action] in the context of a host and
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

            /// The [Task] from which [Actions] are currently being read.
            ///
            /// Passed through to [HostAction].
            task: Option<&'p Task>,

            /// The iterator that yields the [Task]s that TaskIter walks.
            task_iter: std::slice::Iter<'p, Task>,

            /// The iterator that yields the [Action]s from `self.task`. We use these values in
            /// combination with values saved in the `TaskIter` to build [HostActions].
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
                            action
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
    }

    pub mod task {
        use crate::core::{action::Action, manifest::Manifest};
        pub struct Task {
            source: Option<String>,
            name: String,
            user: String,
            actions: Vec<Action>,
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

            pub fn name(&self) -> &str {
                &self.name
            }

            pub fn user(&self) -> &str {
                &self.user
            }

            pub fn actions(&self) -> &[Action] {
                &self.actions
            }

            pub fn vars(&self) -> &[(String, String)] {
                &self.vars
            }
        }

        pub(in crate::core) struct HostTask<'m> {
            /// The [Manifest] from which this [HostTask] was produced.
            ///
            /// This [Manifest] holds any manifest-level variables that must be applied.
            pub manifest: &'m Manifest,

            /// The target host name.
            pub host: &'m str,

            /// The path to the task file that contains the task.
            pub file_path: &'m str,

            /// The [Task] from which this [HostTask] was produced.
            ///
            /// This [Task] holds any task-level variables that must be applied.
            pub task: &'m Task,
        }
    }

    pub mod action {
        use crate::core::{manifest::Manifest, task::Task};
        pub use regex::Regex;

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
                action: &'p Action
            ) -> Self {
                HostAction { host, manifest, task, action, }
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

        impl<'m> HostAction<'m> {
            /// Prepares an [Action] to be sent to a host for execution, e.g. performing variable
            /// substitution.
            pub(in crate::core) fn compile(&self) -> Action {
                todo!()
            }
        }
    }
}
use crate::core::{action::Action, manifest::Manifest, task::Task};

trait ThreadHandle {
    type Error;

    fn join(self) -> Result<(), Self::Error>;
}

use std::any::Any;
impl ThreadHandle for std::thread::JoinHandle<()> {
    type Error = Box<dyn Any + Send + 'static>;
    fn join(self) -> std::thread::Result<()> {
        self.join()
    }
}

trait SendManifest {
    type Error: std::error::Error;
    fn send(&self, manifest: Manifest) -> Result<(), Self::Error>;
}

impl SendManifest for std::sync::mpsc::Sender<Manifest> {
    type Error = std::sync::mpsc::SendError<Manifest>;
    fn send(&self, manifest: Manifest) -> Result<(), Self::Error> {
        self.send(manifest)
    }
}

/// A representation of a thread, and associated values such as communications channels, for
/// communicating with a specific remote host.
///
/// All fields of this struct should be generics with trait bounds. This decouples the code that
/// uses this struct from implementation details such the specific SSH library, possible async
/// runtime, and so on that are used.
struct HostThread<H: ThreadHandle, S: SendManifest> {
    handle: H,
    manifests: S,
}

trait Ssh {
    fn connect() -> Self;

    fn shell();

    fn upload();

    fn download();
}

struct OpenSsh {}

impl Ssh for OpenSsh {
    fn connect() -> Self { todo!() }

    fn shell() {}

    fn upload() {}

    fn download() {}
}


fn main() {
    // Parse arguments, probably using clap.
    todo!();

    let mut manifests: Vec<Manifest> = Vec::new();
    // Load each manifest file specified in the command-line options in the order in which the
    // caller specified them. This also loads tasks, which means errors in any YAML files are
    // caught long before any Actions run.
    todo!();

    let host_threads = spawn_host_threads(&manifests);

    // Send each manifest to each host in its list. (Yes, clone the manifests, creating deep
    // copies. This is intentional and okay.)
    todo!();

    // Join host threads.
    todo!();
}

use std::collections::BTreeMap;
// Collect all hosts from all manifests. Yes, we could do this in a more haphazard way, but
// this is more testable and more obviously correct.
fn spawn_host_threads(manifests: &[Manifest]) -> BTreeMap<String, HostThread<std::thread::JoinHandle<()>, std::sync::mpsc::Sender<Manifest>>> {
    let mut hosts = BTreeMap::new();
    for manifest in manifests {
        // If hosts doesn't already have an entry for the host, then spawn a new thread with a new
        // channel to send to it (as well as any other communications channels that might be
        // required) and store the HostThread tracking all those details in hosts.
        todo!()
    }
    hosts
}
