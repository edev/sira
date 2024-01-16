//! Types for representing individual actions.

#[cfg(doc)]
use crate::core::plan::Plan;
use crate::core::{manifest::Manifest, task::Task};
use regex::{NoExpand, Regex};
use serde::{Deserialize, Serialize};
#[cfg(doc)]
use std::sync::Arc;

/// The types of actions that Sira can perform on a client.
///
/// # (De)serialization
///
/// As of this writing, I am not aware of a way to prevent [serde_yaml] from using YAML tag
/// notation for enums when using them directly. [Task] overrides this by applying
/// `#[serde(with = "serde_yaml::with::singleton_map_recursive")]` to [Task::actions]. If you use
/// [Action] directly, you will run into this limitation. If you know how to resolve it, please
/// open an issue or pull request!
// TODO Try to fix the tagged enum notation issue described above.
// TODO Flesh out Actions. The current states are intentionally basic sketches.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Shell(Vec<String>),

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

impl Action {
    /// Splits a list of [Action]s into as many individual [Action]s as possible.
    ///
    /// For example, an [Action::Shell] can contain many shell commands. To provide the most
    /// granular feedback to the end user, it's best to split these commands into their own
    /// [Action::Shell] values so that they can be processed individually.
    ///
    /// ```
    /// # use sira::core::Action;
    ///
    /// let mut actions = vec![
    ///     Action::Shell(vec!["echo hi".to_owned(), "echo bye".to_owned()]),
    ///     Action::Upload {
    ///         from: ".bashrc".to_owned(),
    ///         to: ".".to_owned(),
    ///     },
    /// ];
    ///
    /// Action::split(&mut actions);
    ///
    /// let mut expected = vec![
    ///     Action::Shell(vec!["echo hi".to_owned()]),
    ///     Action::Shell(vec!["echo bye".to_owned()]),
    ///     Action::Upload {
    ///         from: ".bashrc".to_owned(),
    ///         to: ".".to_owned(),
    ///     },
    /// ];
    /// assert_eq!(expected, actions);
    ///
    /// ```
    pub fn split(list: &mut Vec<Self>) {
        use Action::*;
        let mut output = vec![];
        for source in list.iter() {
            match source {
                Shell(sublist) => output.extend(
                    sublist
                        .iter()
                        .map(|command| Shell(vec![command.to_owned()])),
                ),
                action @ LineInFile { .. } | action @ Upload { .. } | action @ Download { .. } => {
                    output.push(action.to_owned())
                }
            }
        }
        *list = output;
    }
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
/// copies. [HostAction] values get passed throughout the program and across threads, so references
/// are not a great fit.
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
    pub fn new<'plan>(
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

    /// Prepares an [Action] to be sent to a host for execution. Merges manifest and task vars.
    ///
    /// # Variable precedence
    ///
    /// [Task] variables take precedence over [Manifest] variables. For example, if in
    /// [Manifest::vars] you set the variable `breakfast` to be `cake` and in [Task::vars] you set
    /// `breakfast` to be `pie`, the final value of `breakfast` will be `pie`.
    ///
    /// # Variable substitution
    ///
    /// Any variables defined in [Self::manifest] or [Self::task] are interpolated into the
    /// compiled [Action]. There are two forms of variable substitution:
    ///
    /// 1. Simple substitution (`$var`): any occurrence of `$var` is replaced with the variable
    ///    named `var`, if one exists. If `var` does not exist, the [Action] remains unchanged.
    ///    Matching variable names with simple substitution works based on word boundaries, as you
    ///    would expect, so, for instance, `$foobar.baz` matches the variable `foobar` but not any
    ///    of its substrings. If you try to merge a variable `foo` into the string, it will not
    ///    match. In such situations, use braced substitution: `${foo}bar`.
    ///
    /// 2. Braced substitution (`${var}`): any occurrence of `${var}` is substituted with the
    ///    variable named `var`, if one exists. If `var` does not exist, the [Action] remains
    ///    unchanged. This cannot be used recursively; it is a simple text substitution.
    ///
    /// Any portion of an [Action] that runs via `sira-client` may also use shell variables on the
    /// remote host. As long as they do not match the above substitution rules, they will pass
    /// through to the remote host's shell unchanged.
    ///
    /// # Substitution order
    ///
    /// Variables are substituted in the order in which they are defined, and variables defined in
    /// [Manifest::vars] are substituted before variables defined in [Task::vars]. By relying on
    /// this ordering, it is possible to use cascading variable substitutions to a limited degree,
    /// though this generally is not recommended.
    pub fn compile(&self) -> Action {
        let mut action = self.action.clone();

        // To implement variable substitution rules with precedence, we merge variables, in order,
        // and then substitute, again in order.
        let mut vars = self.manifest.vars.clone();
        for (var, value) in &self.task.vars {
            let _ = vars.insert(var.clone(), value.clone());
        }

        // Substitute variables. In order to prevent accidentally recursively substituting
        // variables in some strange corner and edge cases, we use a single regular expression
        // rather than two naive string substitution passes.
        for (var, value) in vars {
            // Form a regular expression that matches $<var> (as a whole word) and ${<var>} where
            // <var> is the name of the variable.
            let pattern = format!(r"\${var}\b|\$\{{{var}}}");
            let regex = Regex::new(&pattern).unwrap();

            // Build an ergonomic regex replacer so we can write DRY code below.
            let replace = |s: &mut String| {
                let _ = std::mem::replace(s, regex.replace_all(s, NoExpand(&value)).into_owned());
            };

            // Run the replacement across all fields of the Action.
            use Action::*;
            match &mut action {
                Shell(commands) => {
                    commands.iter_mut().for_each(replace);
                }
                LineInFile {
                    after,
                    insert,
                    path,
                } => {
                    replace(after);
                    insert.iter_mut().for_each(replace);
                    replace(path);
                }
                Upload { from, to } => {
                    replace(from);
                    replace(to);
                }
                Download { from, to } => {
                    replace(from);
                    replace(to);
                }
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::plan;
    use super::*;
    use indexmap::IndexMap;
    use std::path::PathBuf;

    mod split {
        use super::*;

        #[test]
        fn works() {
            use Action::*;

            // Construct one of each enum variant, and for any variant that might be split,
            // construct one that we expect to be split.
            let mut list = vec![
                Shell(vec!["a".to_string(), "b".to_string()]),
                LineInFile {
                    after: "c".to_string(),
                    insert: vec!["d".to_string(), "e".to_string()],
                    path: "f".to_string(),
                },
                Upload {
                    from: "g".to_string(),
                    to: "h".to_string(),
                },
                Download {
                    from: "i".to_string(),
                    to: "j".to_string(),
                },
            ];

            let expected = vec![
                Shell(vec!["a".to_string()]),
                Shell(vec!["b".to_string()]),
                LineInFile {
                    after: "c".to_string(),
                    insert: vec!["d".to_string(), "e".to_string()],
                    path: "f".to_string(),
                },
                Upload {
                    from: "g".to_string(),
                    to: "h".to_string(),
                },
                Download {
                    from: "i".to_string(),
                    to: "j".to_string(),
                },
            ];

            Action::split(&mut list);

            assert_eq!(expected, list);
        }
    }

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
                    vars: IndexMap::new(),
                };
                HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            }

            #[test]
            #[should_panic(expected = "task does not include this action")]
            fn requires_task_to_include_action() {
                let (_, manifest, task, _) = plan();
                let action = Action::Shell(vec![]);
                HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            }
        }

        #[test]
        fn host_works() {
            let (_, manifest, task, action) = plan();
            let host_action = HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            assert_eq!(&manifest.hosts[0], host_action.host());
        }

        #[test]
        fn manifest_works() {
            let (_, manifest, task, action) = plan();
            let host_action = HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            assert_eq!(&manifest, host_action.manifest());
        }

        #[test]
        fn task_works() {
            let (_, manifest, task, action) = plan();
            let host_action = HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            assert_eq!(&task, host_action.task());
        }

        #[test]
        fn action_works() {
            let (_, manifest, task, action) = plan();
            let host_action = HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
            assert_eq!(&action, host_action.action());
        }

        mod compile {
            use super::*;

            // Compiles an Action without concerning the caller with the details. Returns a String
            // that should have been modified by Compile.
            //
            // `manifest_vars`: variable assignments for Manifest::vars in (key, value) format.
            //
            // `task_vars`: variable assignments for Task::vars in (key, value) format.
            //
            // `action_string`: a string that will be baked into an Action, transformed by compile,
            // and returned.
            fn compile(
                manifest_vars: &[(&'static str, &'static str)],
                task_vars: &[(&'static str, &'static str)],
                action_string: impl Into<String>,
            ) -> String {
                let base = "host_acton_compile_tests".to_owned();

                // transform manifest_vars and task_vars into IndexMaps.
                let manifest_vars = IndexMap::from_iter(
                    manifest_vars
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string())),
                );
                let task_vars = IndexMap::from_iter(
                    task_vars
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string())),
                );

                // Build a Manifest, Task, Action, and HostAction.
                let manifest = Manifest {
                    source: Some(PathBuf::from(base.clone())),
                    name: base.clone(),
                    hosts: vec![base.clone()],
                    include: vec![Task {
                        source: Some(PathBuf::from(base.clone())),
                        name: base.clone(),
                        user: base.clone(),
                        actions: vec![Action::Shell(vec![action_string.into()])],
                        vars: task_vars,
                    }],
                    vars: manifest_vars,
                };
                let task = manifest.include[0].clone();
                let action = task.actions[0].clone();
                let host_action = HostAction {
                    host: "compile-test".to_owned(),
                    manifest,
                    task,
                    action,
                };

                // Compile a new Action and extract a string to test.
                let action = host_action.compile();
                match action {
                    Action::Shell(mut commands) => commands.pop().unwrap(),
                    a => panic!("bug in test fixture. Unexpected action: {a:?}"),
                }
            }

            #[test]
            fn works_on_all_actions() {
                use Action::*;
                let base = "host_acton_compile_tests".to_owned();
                let action_string = "$foo".to_owned();
                let manifest_vars = IndexMap::from([("foo".to_owned(), "bar".to_owned())]);
                let manifest = Manifest {
                    source: Some(PathBuf::from(base.clone())),
                    name: base.clone(),
                    hosts: vec![base.clone()],
                    include: vec![Task {
                        source: Some(PathBuf::from(base.clone())),
                        name: base.clone(),
                        user: base.clone(),
                        actions: vec![
                            Shell(vec![action_string.clone()]),
                            LineInFile {
                                after: action_string.clone(),
                                insert: vec![action_string.clone()],
                                path: action_string.clone(),
                            },
                            Upload {
                                from: action_string.clone(),
                                to: action_string.clone(),
                            },
                            Download {
                                from: action_string.clone(),
                                to: action_string.clone(),
                            },
                        ],
                        vars: IndexMap::new(),
                    }],
                    vars: manifest_vars,
                };
                let task = manifest.include[0].clone();

                let mut host_action = HostAction {
                    host: base,
                    manifest,
                    task: task.clone(),
                    // Placeholder Action; we'll populate this below.
                    action: Shell(vec![]),
                };

                // Call HostAction::compile for each Action variant and test each field.
                let expected_string = "bar".to_owned();
                for action in task.actions {
                    let expected = match action {
                        Shell(_) => Shell(vec![expected_string.clone()]),
                        LineInFile { .. } => LineInFile {
                            after: expected_string.clone(),
                            insert: vec![expected_string.clone()],
                            path: expected_string.clone(),
                        },
                        Upload { .. } => Upload {
                            from: expected_string.clone(),
                            to: expected_string.clone(),
                        },
                        Download { .. } => Download {
                            from: expected_string.clone(),
                            to: expected_string.clone(),
                        },
                    };

                    host_action.action = action;
                    let action = host_action.compile();
                    assert_eq!(expected, action);
                }
            }

            #[test]
            fn merges_manifest_vars() {
                assert_eq!("bar", compile(&[("foo", "bar")], &[], "$foo"));
            }

            #[test]
            fn merges_task_vars() {
                assert_eq!("bar", compile(&[], &[("foo", "bar")], "$foo"));
            }

            #[test]
            fn task_vars_take_precedence() {
                assert_eq!("bar", compile(&[("foo", "foo")], &[("foo", "bar")], "$foo"));
            }

            #[test]
            fn non_matching_vars_are_noop() {
                assert_eq!("noop", compile(&[("foo", "foo")], &[], "noop"));
            }

            #[test]
            fn non_matching_substitutions_are_noop() {
                // Be sure to keep at least one variable so that the for loop runs.
                assert_eq!("$bar", compile(&[("foo", "foo")], &[], "$bar"));
            }

            #[test]
            fn simple_substitution_works_at_end_of_string() {
                assert_eq!("foobar", compile(&[("foo", "bar")], &[], "foo$foo"));
            }

            #[test]
            fn simple_substitution_does_not_match_var_substrings() {
                assert_eq!("$foobar", compile(&[("foo", "bar")], &[], "$foobar"));
            }

            #[test]
            fn braced_substitution_works() {
                assert_eq!("barbar", compile(&[("foo", "bar")], &[], "${foo}bar"));
            }

            #[test]
            fn merges_in_order() {
                assert_eq!(
                    "done",
                    compile(&[("1", "$2"), ("2", "$3"), ("3", "done")], &[], "$1")
                );
            }

            #[test]
            fn merges_task_vars_after_manifest_vars() {
                // Task's "1" should replace Manifest's "1" in place.
                // Task's "3" should come after Manifest's "2".
                // Thus, the replacements should go 1, 2, 3, done.
                assert_eq!(
                    "done",
                    compile(
                        &[("1", "FAIL"), ("2", "$3")],
                        &[("1", "$2"), ("3", "done")],
                        "$1",
                    )
                );
            }
        }
    }
}
