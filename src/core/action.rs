//! Types for representing individual actions.

#[cfg(doc)]
use crate::core::plan::Plan;
use crate::core::{manifest::Manifest, task::Task};
use regex::{NoExpand, Regex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(doc)]
use std::sync::Arc;

/// The relative path to the temporary file that the `sira` and `sira-client` both use when
/// uploading or downloading files.
pub const FILE_TRANSFER_PATH: &str = ".sira-transfer";

pub mod line_in_file;
pub use line_in_file::line_in_file;

/// The types of actions that Sira can perform on a client.
// In order to allow Action to (de)serialize using singleton map notation rather than externally
// tagged notation, we adapt the method used here: https://github.com/dtolnay/serde-yaml/issues/363
//
// This method derives remote definitions for (de)serializing and then manually implements
// Serialize and Deserialize using internal wrapper types that allow us to invoke the methods in
// serde_yaml::with::singleton_map.
//
// TODO Flesh out Actions. The current states are intentionally basic sketches.
// TODO Sort actions alphabetically.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(remote = "Self")]
pub enum Action {
    Shell(Vec<String>),

    /// Replaces a line in a file or inserts a new line.
    ///
    /// # Behavior
    ///
    /// Sira will execute the first matching action from the following list:
    ///
    /// 1. If [line] is already present in the file, Sira will do nothing.
    ///
    /// 1. If [pattern] is set and matches a line in the file, Sira will replace that line's
    ///    contents with [line].
    ///
    /// 1. If [after] is set and matches a line in the file, Sira will insert [line] right after
    ///    the matching line.
    ///
    /// 1. Sira will insert [line] at the end of the file.
    ///
    /// For precise details of how Sira matches file lines against [line], [pattern], and [after],
    /// see the comments on these fields as well as [indent].
    ///
    /// # Line endings and multi-line matching
    ///
    /// [Action::LineInFile] only officially supports Unix-style line endings. If you try to modify
    /// files with non-Unix line endings, you will most likely wind up with mixed line endings or
    /// other issues. This use case has undergone basic testing but is not officially supported.
    /// Proceed at your own risk. If you do encounter bugs with this use case, please feel free to
    /// report them, but they may or may not be fixed.
    ///
    /// Multi-line matching explicitly is not supported and will not work.
    ///
    /// If you have a compelling use case for either of these features, please feel free to open an
    /// issue to discuss adding it.
    ///
    /// # Regular expressions
    ///
    /// [Action::LineInFile] does not support regular expression matching. Most likely, it never
    /// will. There are three reasons for this. First, the author has not personally encountered a
    /// need for such a feature. Second, tools like `sed` may serve this purpose adequately. Third,
    /// supporting regular expressions here would make the interface significantly less ergonomic
    /// and clear.
    ///
    /// If you have a compelling use case for a version of [Action::LineInFile] that supports
    /// regular expressions, please open an issue so that we may discuss it and perhaps design an
    /// appropriate new [Action] to support it, e.g. `Action::RegexInFile`.
    ///
    /// # Special cases
    ///
    /// If the file is empty or contains only [Unicode whitespace], and [line] contains characters
    /// other than [Unicode whitespace], then the file's contents will be replaced with [line]
    /// followed by a newline character.
    ///
    /// If Sira touches the last line in the file, then the resulting file will always end with a
    /// newline character.* Otherwise, the last line will remain unchanged.
    ///
    /// If [pattern] is an empty string, i.e. `Some("".to_string())`, and Sira reaches the step of
    /// matching [pattern], then it will match and replace the first line of the file.
    ///
    /// If [after] is an empty string, and Sira reaches the step of matching [after], then it will
    /// match the first line of the file. For convenience, Sira will insert [line] as the first
    /// line in the file.
    ///
    /// <em>* Sira will actually try to preserve a Mac-style line ending (`\r`) on the final line,
    /// if it finds one, but if this describes your files, then Sira really isn't a good fit for
    /// your use case. For details, please refer to the tests that cover this feature.</em>
    ///
    /// [after]: Self::LineInFile::after
    /// [indent]: Self::LineInFile::indent
    /// [line]: Self::LineInFile::line
    /// [pattern]: Self::LineInFile::pattern
    /// [Unicode whitespace]: https://en.wikipedia.org/wiki/Unicode_character_property#Whitespace
    LineInFile {
        /// The path to the file you wish to modify.
        path: String,

        /// The line to install in the file.
        ///
        /// Matching: Sira checks whether this field matches the file line exactly, subject to the
        /// behavior of [indent]. This check ignores trailing white space on both this field and
        /// the file line.
        ///
        /// [indent]: Self::LineInFile::indent
        line: String,

        /// The line in the file to replace with [line].
        ///
        /// Matching: Sira always matches this field as a substring of the file line.
        ///
        /// [line]: Self::LineInFile::line
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(default)]
        pattern: Option<String>,

        /// The line in the file after which Sira will insert [line].
        ///
        /// Matching: Sira always matches this field as a substring of the file line.
        ///
        /// [line]: Self::LineInFile::line
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(default)]
        after: Option<String>,

        /// Toggles automatic handling of leading white space. Defaults to `true`.
        ///
        /// This field affects both matching and replacing file lines with the goals of matching
        /// existing indentation in a file whenever possible and freeing you from needing to think
        /// about indentation in most situations.
        ///
        /// # Line
        ///
        /// When comparing [line] to a file line, if [indent] is `true`, Sira will strip leading
        /// white space from both values before comparing them.
        ///
        /// # Pattern
        ///
        /// When comparing [pattern] to a file line, Sira checks whether [pattern] is a substring
        /// of the file line. If a line matches, and [indent] is `true`, then Sira will replace any
        /// leading white space in [line] with the file line's leading white space, thereby
        /// matching the existing line's indentation. If [indent] is `false`, then Sira will
        /// replace the file line with [line] as-is.
        ///
        /// # After & default
        ///
        /// When Sira inserts a new line, either by matching [after] or as the default action,
        /// [indent] has no effect.
        ///
        /// If you wish to match indentation in the existing line when possible but also provide
        /// default indentation when inserting a new line, leave [indent] at its default value and
        /// add your desired default indentation to [line].
        ///
        /// [after]: Self::LineInFile::after
        /// [indent]: Self::LineInFile::indent
        /// [line]: Self::LineInFile::line
        /// [pattern]: Self::LineInFile::pattern
        #[serde(skip_serializing_if = "is_true")]
        #[serde(default = "Action::default_indent")]
        indent: bool,
    },

    /// Transfers a file from the control node to managed nodes.
    ///
    /// The transfer takes place in two stages:
    /// 1. Sira transfers the file on the control node to a temporary file owned by the Sira user
    ///    in the Sira user's home directory (or wherever OpenSSH `scp` sessions start).
    /// 1. Then, Sira invokes `sira-client` on the managed node to change the file's owner
    ///    (i.e. user), group, and permissions and move it into place.
    ///
    /// # Security considerations
    ///
    /// When the file is initially transferred to the managed node, it will be in the Sira user's
    /// home directory with default permissions. If you need to protect files from being prying
    /// eyes during this stage, you have several options. First, you may wish to restrict the Sira
    /// user's home directory, e.g. to `700` or `770` permissions. Second, you may choose to store
    /// the file in encrypted form on the control node and decrypt it on the managed hode after
    /// transferring it, perhaps using [Action::Shell] to run the decryption while storing the
    /// decryption key securely on the control node.
    Upload {
        /// The path to the source file, i.e. the file on the control node.
        ///
        /// This path may be relative or absolute. If the path is relative, it is relative to the
        /// directory from which you invoke Sira, **not** the task file that contains the action.
        from: String,

        /// The final path of the file on the managed node.
        ///
        /// This path may be relative or absolute. If the path is relative, it is relative to the
        /// directory in which `ssh` sessions for the Sira user start; this is usually the Sira
        /// user's home directory.
        ///
        /// The parent directory must exist; if it does not exist, this [Action] will fail.
        to: String,

        /// The final owner of the file on the managed node. Defaults to `root`.
        #[serde(skip_serializing_if = "Action::user_or_group_is_default")]
        #[serde(default = "Action::default_user_and_group")]
        user: String,

        /// The final group of the file on the managed node. Defaults to `root`.
        #[serde(skip_serializing_if = "Action::user_or_group_is_default")]
        #[serde(default = "Action::default_user_and_group")]
        group: String,

        /// The final permissions of the file on the managed node, in any form that `chmod` will
        /// accept. If this value is unspecified, then `chmod` will not be run, and the file will
        /// have the Sira user's default permissions. (Note that these might vary from the final
        /// user's default permissions.)
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(default)]
        permissions: Option<String>,

        /// Whether to overwrite an existing file at [Action::Upload::to]. Defaults to `true`.
        ///
        /// If `true`, this option causes Sira to invoke `mv -n` instead of `mv`. Sira's behavior
        /// follows from your system's implementation of `mv -n`: most likely, in the event that a
        /// file exists at the destination, Sira will silently decline to move the file.
        ///
        /// If this property is `false` and the file already exists, then the user, group, and
        /// permissions **will not be updated**. The existing file will remain untouched.
        #[serde(skip_serializing_if = "is_true")]
        #[serde(default = "Action::default_overwrite")]
        overwrite: bool,
    },

    // I need to add more fields, like user, group, and permissions.
    Download {
        from: String,
        to: String,
    },
}

// Adapted from https://github.com/dtolnay/serde-yaml/issues/363. See comment on Action for more.
impl Serialize for Action {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct ExternallyTaggedAction<'a>(&'a Action);
        impl<'a> Serialize for ExternallyTaggedAction<'a> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                Action::serialize(self.0, serializer)
            }
        }
        serde_yaml::with::singleton_map::serialize(&ExternallyTaggedAction(self), serializer)
    }
}

// Adapted from https://github.com/dtolnay/serde-yaml/issues/363. See comment on Action for more.
impl<'de> Deserialize<'de> for Action {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ExternallyTaggedAction(Action);
        impl<'de> Deserialize<'de> for ExternallyTaggedAction {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Ok(ExternallyTaggedAction(Action::deserialize(deserializer)?))
            }
        }
        let eta: ExternallyTaggedAction =
            serde_yaml::with::singleton_map::deserialize(deserializer)?;
        Ok(eta.0)
    }
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
    ///         user: "root".to_owned(),
    ///         group: "root".to_owned(),
    ///         permissions: None,
    ///         overwrite: false,
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
    ///         user: "root".to_owned(),
    ///         group: "root".to_owned(),
    ///         permissions: None,
    ///         overwrite: false,
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

    /// Provides the default indentation value when deserializing.
    fn default_indent() -> bool {
        true
    }

    const DEFAULT_USER_AND_GROUP: &'static str = "root";

    /// Provides the default user and group when deserializing.
    fn default_user_and_group() -> String {
        Self::DEFAULT_USER_AND_GROUP.to_string()
    }

    /// Tells serde whether to skip serializing a user or group (because it's the default value).
    fn user_or_group_is_default(value: &str) -> bool {
        value == Self::DEFAULT_USER_AND_GROUP
    }

    /// Provides the default value for [Action::Upload::overwrite] when deserializing.
    fn default_overwrite() -> bool {
        true
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
                    path,
                    line,
                    pattern,
                    after,
                    indent: _,
                } => {
                    replace(path);
                    replace(line);
                    pattern.as_mut().map(replace);
                    after.as_mut().map(replace);
                }
                Upload {
                    from,
                    to,
                    user,
                    group,
                    permissions,
                    overwrite: _,
                } => {
                    replace(from);
                    replace(to);
                    replace(user);
                    replace(group);
                    permissions.as_mut().map(replace);
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

/// Trivial function for use with `skip_serializing_if`.
fn is_true(var: &bool) -> bool {
    *var
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::plan;
    use super::*;
    use indexmap::IndexMap;
    use std::path::PathBuf;

    mod action {
        use super::*;
        mod serde {
            use super::*;

            fn check(yaml: &str, action: Action) {
                assert_eq!(yaml, serde_yaml::to_string(&action).unwrap());
                assert_eq!(action, serde_yaml::from_str(yaml).unwrap());
            }

            mod shell {
                use super::*;

                #[test]
                fn works() {
                    let yaml = "\
                        shell:\n\
                        - echo hi\n\
                        - echo bye\n";
                    let action = Action::Shell(vec!["echo hi".to_string(), "echo bye".to_string()]);
                    check(yaml, action);
                }

                #[test]
                #[should_panic(expected = "expected a sequence")]
                fn requires_a_list() {
                    // It would be awesome to be able to write:
                    //
                    //     shell: echo hi
                    //
                    // instead of:
                    //
                    //     shell:
                    //     - echo hi
                    //
                    // However, at this time, this is not implemented. This test documents and
                    // verifies the current state of implementation.
                    let yaml = "shell: echo hi";
                    let _: Action = serde_yaml::from_str(yaml).unwrap();
                }
            }

            mod line_in_file {
                use super::*;

                #[test]
                fn works() {
                    let yaml = "\
line_in_file:
  path: a
  line: b
  pattern: c
  after: d
  indent: false\n";
                    let action = Action::LineInFile {
                        path: "a".to_string(),
                        line: "b".to_string(),
                        pattern: Some("c".to_string()),
                        after: Some("d".to_string()),
                        indent: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn pattern_defaults_to_none() {
                    let yaml = "\
line_in_file:
  path: a
  line: b
  after: d
  indent: false\n";
                    let action = Action::LineInFile {
                        path: "a".to_string(),
                        line: "b".to_string(),
                        pattern: None,
                        after: Some("d".to_string()),
                        indent: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn after_defaults_to_none() {
                    let yaml = "\
line_in_file:
  path: a
  line: b
  pattern: c
  indent: false\n";
                    let action = Action::LineInFile {
                        path: "a".to_string(),
                        line: "b".to_string(),
                        pattern: Some("c".to_string()),
                        after: None,
                        indent: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn indent_defaults_to_true() {
                    let yaml = "\
line_in_file:
  path: a
  line: b
  pattern: c
  after: d\n";
                    let action = Action::LineInFile {
                        path: "a".to_string(),
                        line: "b".to_string(),
                        pattern: Some("c".to_string()),
                        after: Some("d".to_string()),
                        indent: true,
                    };
                    check(yaml, action);
                }
            }

            mod upload {
                use super::*;

                #[test]
                fn works() {
                    let yaml = "\
upload:
  from: a
  to: b
  user: c
  group: d
  permissions: e
  overwrite: false\n";
                    let action = Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                        user: "c".to_string(),
                        group: "d".to_string(),
                        permissions: Some("e".to_string()),
                        overwrite: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn user_defaults_to_root() {
                    let yaml = "\
upload:
  from: a
  to: b
  group: d
  permissions: e
  overwrite: false\n";
                    let action = Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                        user: "root".to_string(),
                        group: "d".to_string(),
                        permissions: Some("e".to_string()),
                        overwrite: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn group_defaults_to_root() {
                    let yaml = "\
upload:
  from: a
  to: b
  user: c
  permissions: e
  overwrite: false\n";
                    let action = Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                        user: "c".to_string(),
                        group: "root".to_string(),
                        permissions: Some("e".to_string()),
                        overwrite: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn permissions_defaults_to_none() {
                    let yaml = "\
upload:
  from: a
  to: b
  user: c
  group: d
  overwrite: false\n";
                    let action = Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                        user: "c".to_string(),
                        group: "d".to_string(),
                        permissions: None,
                        overwrite: false,
                    };
                    check(yaml, action);
                }

                #[test]
                fn overwrite_defaults_to_true() {
                    let yaml = "\
upload:
  from: a
  to: b
  user: c
  group: d
  permissions: e\n";
                    let action = Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                        user: "c".to_string(),
                        group: "d".to_string(),
                        permissions: Some("e".to_string()),
                        overwrite: true,
                    };
                    check(yaml, action);
                }
            }

            // TODO Add unit tests for Download once it's fully fleshed out.
        }
    }

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
                    path: "a".to_string(),
                    line: "b".to_string(),
                    pattern: Some("c".to_string()),
                    after: Some("d".to_string()),
                    indent: false,
                },
                Upload {
                    from: "g".to_string(),
                    to: "h".to_string(),
                    user: "h".to_string(),
                    group: "i".to_string(),
                    permissions: Some("j".to_string()),
                    overwrite: true,
                },
                Download {
                    from: "k".to_string(),
                    to: "l".to_string(),
                },
            ];

            let expected = vec![
                Shell(vec!["a".to_string()]),
                Shell(vec!["b".to_string()]),
                LineInFile {
                    path: "a".to_string(),
                    line: "b".to_string(),
                    pattern: Some("c".to_string()),
                    after: Some("d".to_string()),
                    indent: false,
                },
                Upload {
                    from: "g".to_string(),
                    to: "h".to_string(),
                    user: "h".to_string(),
                    group: "i".to_string(),
                    permissions: Some("j".to_string()),
                    overwrite: true,
                },
                Download {
                    from: "k".to_string(),
                    to: "l".to_string(),
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
                                path: action_string.clone(),
                                line: action_string.clone(),
                                pattern: Some(action_string.clone()),
                                after: Some(action_string.clone()),
                                indent: true,
                            },
                            Upload {
                                from: action_string.clone(),
                                to: action_string.clone(),
                                user: action_string.clone(),
                                group: action_string.clone(),
                                permissions: Some(action_string.clone()),
                                overwrite: true,
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
                            path: expected_string.clone(),
                            line: expected_string.clone(),
                            pattern: Some(expected_string.clone()),
                            after: Some(expected_string.clone()),
                            indent: true,
                        },
                        Upload { .. } => Upload {
                            from: expected_string.clone(),
                            to: expected_string.clone(),
                            user: expected_string.clone(),
                            group: expected_string.clone(),
                            permissions: Some(expected_string.clone()),
                            overwrite: true,
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
