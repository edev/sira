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
        use crate::core::action::Action;
        use crate::core::manifest::{Manifest, TaskIter};
        use crate::core::task::{ActionIter, HostTask, Task};
        use std::path::Path;

        /// A plan of action for executing a given list of manifests.
        ///
        /// This struct constitutes the public interface that executors use to interact with
        /// [Manifest]s, [Task]s, and [Action]s on the controller.
        pub struct Plan {
            /// The official, ordered list of manifests that comprise the plan.
            ///
            /// Everything else can be computed from these manifests.
            manifests: Vec<Manifest>, // TODO Add file paths to this list or otherwise devise a
                                      // coherent plan for tracking file names across manifest and
                                      // task files.
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
            pub fn plan_for(&self, host: &str) -> HostPlan {
                todo!()
            }
        }

        pub struct HostPlan<'p> {
            /// The host on which this plan will run.
            host: String,

            /// The [Plan] to run on the host.
            plan: &'p Plan,
        }

        impl<'p> HostPlan<'p> {
            pub fn iter(&self) -> HostPlanIter {
                todo!()
            }
        }

        pub struct HostPlanIter<'p> {
            host: &'p str,

            /// An iterator that yields the next manifest to process.
            manifests: std::slice::Iter<'p, Manifest>,

            /// The manifest over which this HostPlan is currently iterating.
            ///
            /// This will be [None] before iteration starts and after iteration finishes.
            current_manifest: Option<&'p Manifest>,

            /// An iterator that yields the next [Task] to process. Generated from
            /// [current_manifest].
            tasks: Option<TaskIter>,

            /// The task file over which this HostPlan is currently iterating.
            ///
            /// This will be [None] before iteration starts and after iteration finishes.
            current_task: Option<HostTask<'p>>,

            /// An iterator that yields the next [HostAction] to process.
            ///
            /// This will be [None] before iteration starts and after iteration finishes. Generated
            /// from [current_task].
            actions: Option<ActionIter>,
        }

        impl<'p> Iterator for HostPlanIter<'p> {
            type Item = Action;

            fn next(&mut self) -> Option<Self::Item> {
                // Runs through manifests, tasks, and actions like hour, minute, and second hands
                // of a clock, advancing the larger value only once the next smaller iterator is
                // done. Calls HostAction::compile to generate and yield a final Action.
                todo!()
            }
        }

        // This function is meant as the easy, default entry point for executors.
        //
        // This function doesn't actually know how to parse; it simply calls
        // core::manifest::load_manifests for each file.
        //
        // The return type should be a Result<Self, Error>, but I haven't defined the error yet.
        pub fn from_manifest_files(files: &[impl AsRef<Path>]) -> Plan {
            todo!()
        }
    }

    pub mod manifest {
        use crate::core::task::Task;
        pub struct Manifest {
            name: String,
            hosts: Vec<String>,
            include: Vec<(String, Vec<Task>)>, // (file_path, actions)
            vars: Vec<(String, String)>,
        }

        pub fn load_manifests<R: std::io::BufRead>(source: R) -> Vec<Manifest> {
            todo!()
        }

        impl Manifest {
            pub(in crate::core) fn tasks_for(&self, host: &str) -> TaskIter {
                todo!()
            }
        }

        /// Iterates over [Task]s in a [Manifest].
        ///
        /// # Returns
        ///
        /// [HostTask] values representing a given [Task] in the context of a host and [Manifest].
        pub(in crate::core) struct TaskIter {

        }
    }

    pub mod task {
        use crate::core::{action::Action, manifest::Manifest};
        pub struct Task {
            name: String,
            user: String,
            actions: Vec<Action>,
            vars: Vec<(String, String)>,
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

        impl<'m> HostTask<'m> {
            pub(in crate::core) fn actions_for(&'m self, host: &'m str, manifest_vars: &'m [(String, String)]) -> ActionIter {
                todo!()
            }
        }


        /// Iterates over [Action]s in a [Task].
        ///
        /// # Returns
        ///
        /// [HostAction] values representing a given [Action] in the context of a host, [Manifest],
        /// and [Task].
        pub(in crate::core) struct ActionIter {
        }
    }

    pub mod action {
        use crate::core::{manifest::Manifest, task::HostTask};
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

        pub(in crate::core) struct HostAction<'m> {

            /// The target host name.
            host: &'m str,

            /// The [HostTask] that generated this HostAction.
            host_task: &'m HostTask<'m>,

            /// The manifest that caused this [Action] to run.
            ///
            /// You can access this same value via `self.host_task.manifest`.
            manifest: &'m Manifest,

            /// The original [Action] from the [Task]. It is not yet ready to run!
            action: &'m Action,
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
