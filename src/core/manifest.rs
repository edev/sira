//! Types for representing manifest files.
use crate::core::action::{Action, HostAction};
#[cfg(doc)]
use crate::core::plan::Plan;
use crate::core::task::Task;
use crate::crypto;
use anyhow::{anyhow, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Deserializer;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

/// The name of the allowed signers file used to verify manifest and task files.
pub const ALLOWED_SIGNERS_FILE: &str = "manifest";

/// Returns whether the allowed signers file for verifying manifest and task files is installed.
///
/// The first time this function is called, it checks for the allowed signers file and stores the
/// result. For the remainder of the program's execution, this is the official answer.
///
/// If this function encounters an error while trying to check whether the file the file exists,
/// it returns the error without storing a value. The caller should most likely quit at this point.
/// However, if execution continues, the next call to this function will retry the above logic.
pub fn allowed_signers_file_installed() -> anyhow::Result<bool> {
    static mut INSTALLED: bool = true;
    static ONCE: Once = Once::new();

    let installed = crypto::allowed_signers_installed(ALLOWED_SIGNERS_FILE)?;

    // Safety: Once::call_once() safely synchronizes the mutation of INSTALLED, similarly to an
    // RwLock. Using OnceLock here would obviate the need for an unsafe block but instead introduce
    // the possibility of logic errors: OnceLock::take can clear the stored value, and we want to
    // explicitly disallow this.
    unsafe {
        ONCE.call_once(|| INSTALLED = installed);
        Ok(INSTALLED)
    }
}

/// Handles verification for a manifest or task file.
///
/// `source` is the path to the file to verify; `source_file` must be the contents of this file.
/// Passing these in together guarantees that the caller has and keeps the canonical and verified
/// version of the file. This prevents TOCTOU issues (e.g. from loading the file twice).
///
/// `file_type` is a user-friendly description of the type of file, e.g. `"manifest"` or `"task"`.
///
/// If the allowed signers file is installed, then all files must pass signature verification.
/// Otherwise, all files must be unsigned: if we detect a signature but have no allowed signers
/// file with which to verify it, then verification fails. Returns [Ok] if the caller can keep
/// running or [Err] if the caller must exit.
fn verify(source: impl AsRef<Path>, source_file: &[u8], file_type: &str) -> anyhow::Result<()> {
    let signature_path = crypto::signature_path(&source);

    if allowed_signers_file_installed()? {
        return crypto::verify(source_file, signature_path, ALLOWED_SIGNERS_FILE, "sira");
    }

    if signature_path.try_exists()? {
        bail!(
            "Signed {} file detected. Please install the manifest allowed signers file:\n  {}",
            file_type,
            crypto::allowed_signers_path(ALLOWED_SIGNERS_FILE)?.to_string_lossy(),
        );
    }

    Ok(())
}

/// Loads [Manifest] values from a manifest file.
///
/// Verifies the signatures on `source` and any task files that `source` includes.
// TODO Ensure that this function properly sets base_path for each manifest file so that tasks are
// relative to each file rather than the first file. Add this to the test suite.
pub fn load_manifests(source: impl AsRef<Path>) -> anyhow::Result<Vec<Manifest>> {
    let mut manifests = vec![];
    let source_file = fs::read(&source)?;

    verify(&source, &source_file, "manifest")?;

    // Strip the file name from source to create the base path. Included task files with relative
    // paths will be relative to this base path.
    let base_path = source.as_ref().parent().ok_or(anyhow!(
        "could not compute parent directory for path: {:?}",
        source.as_ref(),
    ))?;

    for document in Deserializer::from_slice(&source_file) {
        let manifest_file = ManifestFile::deserialize(document)?;
        let include = load_includes(base_path, manifest_file.include)?;

        let manifest = Manifest {
            source: Some(source.as_ref().to_path_buf()),
            name: manifest_file.name,
            hosts: manifest_file.hosts,
            include,
            vars: manifest_file.vars,
        };
        manifests.push(manifest);
    }
    Ok(manifests)
}

/// Loads [Task]s from a [ManifestFile::include] list of file names.
///
/// This is a private method meant for use by [load_manifests].
fn load_includes(base_path: &Path, includes: Vec<String>) -> anyhow::Result<Vec<Task>> {
    let mut tasks = vec![];
    for task_file in includes {
        let path = base_path.join(&task_file);
        let source_file = fs::read(&path)?;
        verify(&path, &source_file, "task")?;
        tasks.extend(load_tasks(path, &source_file)?);
    }
    Ok(tasks)
}

/// Loads [Task]s from a single file.
///
/// This is a private method meant for use by [load_manifests].
fn load_tasks(source: impl AsRef<Path>, source_file: &[u8]) -> anyhow::Result<Vec<Task>> {
    let mut tasks = vec![];
    for document in Deserializer::from_slice(source_file) {
        let mut task = Task::deserialize(document)?;
        task.source = Some(source.as_ref().to_path_buf());

        // Deserializing produces actions in their most direct representations from the source, but
        // we want to ensure that actions are split up into the smallest chunks possible so that
        // the user gets the most granular feedback we can provide.
        task.split_actions();

        tasks.push(task);
    }
    Ok(tasks)
}

/// Represents a manifest file; typically used in the context of a [Plan].
///
/// This type is typically parsed from a manifest file, but it can be constructed programmatically
/// as well.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Manifest {
    /// Where this manifest came from.
    ///
    /// For instance, a manifest loaded from a file should set this to the path to the file.
    ///
    /// For manifests from other sources, e.g. directly from Rust or from network sources,
    /// there is currently no standard value to place here, because these are not intended
    /// use cases for Sira at this time.
    #[serde(skip)]
    pub source: Option<PathBuf>,

    /// The [Manifest]'s name. Used for informational, logging, and debugging purposes.
    pub name: String,

    /// The list of hosts on which this manifest will run.
    ///
    /// Order is perserved from the source file but is typically unimportant.
    pub hosts: Vec<String>,

    /// [Task]s (typically loaded from task files) that comprise this manifest.
    ///
    /// Order is preserved from the source file. Tasks are executed in order.
    ///
    /// When loading [Manifest] values loaded from files, any included task file paths are relative
    /// to the manifest files that contain them.
    pub include: Vec<Task>,

    /// [Manifest]-level variables, which will eventually be compiled when actions are run.
    ///
    /// Variables are stored as `(name, value)` tuples.
    ///
    /// Order is preserved from the source file but is typically unimportant.
    #[serde(skip_serializing_if = "IndexMap::is_empty", default)]
    pub vars: IndexMap<String, String>,
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

/// A [Manifest] loaded directly from a file (or other deserialized input).
///
/// This type is constructed during file loading, e.g. from [load_manifests]. It is a stepping
/// stone between input files and fully loaded [Manifest]s.
///
/// Identical to [Manifest] except that [ManifestFile::include] is a list of file names rather than
/// a list of [Task]s.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ManifestFile {
    /// Same as [Manifest::source].
    #[serde(skip)]
    pub source: Option<String>,

    /// Same as [Manifest::name].
    pub name: String,

    /// Same as [Manifest::hosts].
    pub hosts: Vec<String>,

    /// A list of files from which to load [Task]s. Once you have loaded them, you can construct a
    /// full and complete [Manifest].
    ///
    /// Order is preserved from the source file.
    ///
    /// If you are implementing some novel form of manifest and task loading, you can safely
    /// store arbitrary values here as part of your loading code.
    ///
    /// # Non-Unicode paths
    ///
    /// While Rust supports non-Unicode paths, this field intentionally does not. The paths here
    /// are presumably under a system adminstrator's control, so requiring them to be Unicode seems
    /// reasonable. Meanwhile, someone developing an alternative loading scheme seems likely to
    /// find [String]s more useful than [Path]s or [PathBuf]s.
    ///
    /// [Path]: std::path::Path
    /// [PathBuf]: std::path::PathBuf
    pub include: Vec<String>,

    /// Same as [Manifest::vars].
    #[serde(skip_serializing_if = "IndexMap::is_empty", default)]
    pub vars: IndexMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::plan;
    use super::*;

    // load_manifests surfaces any errors it encounters, and much of the complex work it does is
    // through code that's already under test elsewhere, so we only have to test the happy path and
    // signature verification.
    mod load_manifests {
        use super::*;

        #[test]
        fn works() {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources/test/load_manifests/manifest1.yaml");
            let manifests = load_manifests(source).unwrap();

            let expected = vec![
                Manifest {
                    source: Some(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("resources/test/load_manifests/manifest1.yaml"),
                    ),
                    name: "desktops".to_owned(),
                    hosts: vec!["t470".to_owned(), "zen3".to_owned()],
                    include: vec![
                        Task {
                            source: Some(
                                Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("resources/test/load_manifests/task1.yaml"),
                            ),
                            name: "apt install".to_owned(),
                            user: "root".to_owned(),
                            actions: vec![
                                Action::Shell(vec!["apt upgrade".to_owned()]),
                                Action::Shell(vec!["apt install -y $packages".to_owned()]),
                            ],
                            vars: [(
                                "packages".to_owned(),
                                "aptitude build-essential exa".to_owned(),
                            )]
                            .into(),
                        },
                        Task {
                            source: Some(
                                Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("resources/test/load_manifests/task2.yaml"),
                            ),
                            name: "snap install".to_owned(),
                            user: "root".to_owned(),
                            actions: vec![Action::Shell(vec!["snap install $snaps".to_owned()])],
                            vars: [("snaps".to_owned(), "discord".to_owned())].into(),
                        },
                    ],
                    vars: [
                        ("alpha".to_owned(), "a".to_owned()),
                        ("beta".to_owned(), "b".to_owned()),
                    ]
                    .into(),
                },
                Manifest {
                    source: Some(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("resources/test/load_manifests/manifest1.yaml"),
                    ),
                    name: "t470".to_owned(),
                    hosts: vec!["t470".to_owned()],
                    include: vec![Task {
                        source: Some(
                            Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("resources/test/load_manifests/t470.yaml"),
                        ),
                        name: "set host name".to_owned(),
                        user: "root".to_owned(),
                        actions: vec![Action::Shell(vec!["hostnamectl hostname t470".to_owned()])],
                        vars: IndexMap::new(),
                    }],
                    vars: IndexMap::new(),
                },
                Manifest {
                    source: Some(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("resources/test/load_manifests/manifest1.yaml"),
                    ),
                    name: "zen3".to_owned(),
                    hosts: vec!["zen3".to_owned()],
                    include: vec![Task {
                        source: Some(
                            Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("resources/test/load_manifests/zen3.yaml"),
                        ),
                        name: "set host name".to_owned(),
                        user: "root".to_owned(),
                        actions: vec![Action::Shell(vec!["hostnamectl hostname zen3".to_owned()])],
                        vars: IndexMap::new(),
                    }],
                    vars: IndexMap::new(),
                },
            ];

            assert_eq!(expected, manifests);
        }

        mod verifies_manifest_file {
            use super::*;

            // For security, we hard code the allowed_signers file name. This means we can't test
            // how load_manifests() works when the key is not installed, with or without a file
            // signature to check.
            //
            // We also don't need to test the happy path here, because the presence of the
            // allowed_signers file requires the basic happy path test for load_manifests() to
            // successfully verify the input files' signatures.

            #[test]
            #[should_panic(expected = "missing signature file")]
            fn missing_signature() {
                let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/test/load_manifests/unsigned.manifest");
                load_manifests(source).unwrap();
            }

            #[test]
            #[should_panic(expected = "incorrect signature")]
            fn bad_signature() {
                let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/test/load_manifests/bad-signature.manifest");
                load_manifests(source).unwrap();
            }
        }

        mod verifies_task_files {
            use super::*;

            // The same caveats as above apply here as well.

            #[test]
            #[should_panic(expected = "missing signature file")]
            fn missing_signature() {
                // The first task file in this manifest is signed, but the second is not;
                // verification must fail.
                let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/test/load_manifests/unsigned-tasks.manifest");
                load_manifests(source).unwrap();
            }

            #[test]
            #[should_panic(expected = "incorrect signature")]
            fn bad_signature() {
                let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/test/load_manifests/bad-task-signature.manifest");
                load_manifests(source).unwrap();
            }
        }
    }

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
                Action::Shell(vec!["echo hi".into(), "pwd".into()]),
                Action::LineInFile {
                    path: "/etc/hosts".into(),
                    line: "192.168.1.93 zen3".into(),
                    pattern: None,
                    after: Some("localhost".into()),
                    indent: true,
                },
                Action::Upload {
                    from: "from".into(),
                    to: "to".into(),
                    user: "user".into(),
                    group: "group".into(),
                    permissions: Some("777".into()),
                    overwrite: true,
                },
            ];

            // Task 2 has no actions.

            // Actions for Task 3 (below).
            let task_3_actions = vec![Action::Shell(vec!["echo bye".into(), "logout".into()])];

            let tasks = vec![
                // A normal, routine task.
                Task {
                    source: None,
                    name: "Task 1".into(),
                    user: "george".into(),
                    actions: task_1_actions.clone(),
                    vars: IndexMap::new(),
                },
                // A corner case: a task that's empty.
                Task {
                    source: None,
                    name: "Task 2".into(),
                    user: "george".into(),
                    actions: vec![],
                    vars: IndexMap::new(),
                },
                // Another routine task afterward.
                Task {
                    source: None,
                    name: "Task 3".into(),
                    user: "george".into(),
                    actions: task_3_actions.clone(),
                    vars: IndexMap::new(),
                },
            ];

            let manifest = Manifest {
                source: None,
                name: "API test".into(),
                hosts: vec!["api_test".into()],
                include: tasks,
                vars: IndexMap::new(),
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
                vars: IndexMap::new(),
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
                vars: IndexMap::new(),
            };

            let manifest = Manifest {
                source: None,
                name: "API test".into(),
                hosts: vec!["api_test".into()],
                include: vec![task],
                vars: IndexMap::new(),
            };

            let mut task_iter = manifest.tasks_for("api_test").unwrap();
            assert!(task_iter.next().is_none());

            let mut task_into_iter = manifest.into_tasks_for("api_test").unwrap();
            assert!(task_into_iter.next().is_none());
        }
    }
}
