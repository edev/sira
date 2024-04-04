//! Verifies (de)serialization of Manifest, Task, and Action values to/from YAML.
//!
//! This module is extensively documented, since the tests are nuanced.
//!
//! Actions are not tested on their own here; they are used in the context of Manifests and
//! Tasks.

use indexmap::IndexMap;
use sira::core::manifest::ManifestFile;
use sira::core::*;
use std::path::PathBuf;
use std::str::FromStr;

/// Stores a pair of possibly-overridden fields, one in YAML and one in Rust.
///
/// See Overrides for details.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Override<Y, V> {
    yaml: Y,
    value: V,
}

/// For internal use by Overrides. Creates override setters.
///
/// Given `setter!(<field_name>, <type>)`, creates a setter of the form:
///
/// `fn <field_name>(mut self, overrides: Override<&'static str, <type>> -> Self`
///
/// The resulting function sets overrides for `field_name`. It is not possible to clear an
/// override with a setter function, i.e. you cannot use a setter to set Override::yaml or
/// an Override::value to None.
macro_rules! setter {
    ($field:ident, $type:ty) => {
        #[allow(dead_code)]
        fn $field(mut self, overrides: Override<&'static str, $type>) -> Self {
            self.$field = Override {
                yaml: Some(overrides.yaml),
                value: Some(overrides.value),
            };
            self
        }
    };
}

mod manifest {
    use super::*;

    /// Stores any values you might wish to override from default when creating a YAML-Manifest
    /// pair.
    ///
    /// Internally, each field has an outer Option indicating whether it is overridden: a Some
    /// value indicates an override, and None indicates that you wish to use the default value.
    /// String and Option<String> fields in Manifests are converted to `&'static str` for easier
    /// construction of tests.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Overrides {
        source: Override<Option<&'static str>, Option<Option<&'static str>>>,
        name: Override<Option<&'static str>, Option<&'static str>>,
        hosts: Override<Option<&'static str>, Option<Vec<&'static str>>>,
        include: Override<Option<&'static str>, Option<Vec<Task>>>,
        vars: Override<Option<&'static str>, Option<IndexMap<String, String>>>,
    }

    impl Overrides {
        /// Returns a default Override with all values set to None.
        fn new() -> Self {
            Self {
                source: Override::default(),
                name: Override::default(),
                hosts: Override::default(),
                include: Override::default(),
                vars: Override::default(),
            }
        }

        setter!(source, Option<&'static str>);
        setter!(name, &'static str);
        setter!(hosts, Vec<&'static str>);
        setter!(include, Vec<Task>);
        setter!(vars, IndexMap<String, String>);

        /// Generates a YAML-Manifest value pair using Overrides, deserializes the YAML, and
        /// asserts_eq! on the generated Manifest and the deserialized Manifest.
        fn assert_de(self) {
            let (source, manifest) = self.source_manifest_pair();
            assert_eq!(manifest, serde_yaml::from_str::<Manifest>(&source).unwrap());
        }

        /// Generates a YAML-Manifest value pair using Overrides, serializes the Manifest, and
        /// asserts_eq! on the generated YAML and the serialized YAML.
        fn assert_ser(self) {
            let (source, manifest) = self.source_manifest_pair();
            assert_eq!(source, serde_yaml::to_string(&manifest).unwrap());
        }

        /// Manually generates a correct pairing of a YAML String and a Manifest.
        ///
        /// By default (i.e. with a newly minted Overrides value), produces values representing the
        /// most basic test case.
        ///
        /// To override a field, use the methods provided on Overrides.
        fn source_manifest_pair(self) -> (String, Manifest) {
            // Pull an owned String from the user-friendly `&'static str` that Overrides stores.
            // Add a newline if the value is non-empty.
            let source_yaml = match self.source.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "".to_owned(),
            };

            // Extract an owned String from Overrides, if applicable.
            let source = match self.source.value {
                Some(v) => v.map(|s| PathBuf::from_str(s).unwrap()),
                None => None,
            };

            // The rest of the values follow the pattern above.

            let name_yaml = match self.name.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "name: Console commands\n".to_owned(),
            };
            let name = match self.name.value {
                Some(v) => v.to_owned(),
                None => "Console commands".to_owned(),
            };

            let hosts_yaml = match self.hosts.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "hosts:\n- bob\n".to_owned(),
            };
            let hosts = match self.hosts.value {
                Some(v) => v.into_iter().map(str::to_owned).collect(),
                None => vec!["bob".to_owned()],
            };

            // Note on permissions: if the string looks like a number, serde_yaml will quote it,
            // e.g. "640" -> "'640'", when serializing. Deserializing unquoted numbers works just
            // fine, but I'm not aware of a way to turn this quoting off. The only place we
            // serialize, other than in tests, is generating Actions to send to sira-client, so
            // this is never a user-facing issue.
            let include_yaml = match self.include.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "\
include:
- name: Console commands
  user: bob
  actions:
  - shell:
    - pwd
    - echo $message > ./message
    - uname -r
  vars:
    message: Hello there!
- name: Transfer config files
  user: bob
  actions:
  - upload:
      from: ./files/home/bob/.zshrc
      to: .
      user: dave
      group: buster
      permissions: ugo=rwx
"
                .to_owned(),
            };
            let include = match self.include.value {
                Some(v) => v,
                None => {
                    let mut task1_vars = IndexMap::new();
                    task1_vars.insert("message".to_owned(), "Hello there!".to_owned());
                    vec![
                        Task {
                            source: None,
                            name: "Console commands".to_owned(),
                            user: "bob".to_owned(),
                            actions: vec![Action::Shell(vec![
                                "pwd".to_owned(),
                                "echo $message > ./message".to_owned(),
                                "uname -r".to_owned(),
                            ])],
                            vars: task1_vars,
                        },
                        Task {
                            source: None,
                            name: "Transfer config files".to_owned(),
                            user: "bob".to_owned(),
                            actions: vec![Action::Upload {
                                from: "./files/home/bob/.zshrc".to_owned(),
                                to: ".".to_owned(),
                                user: "dave".to_owned(),
                                group: "buster".to_owned(),
                                permissions: Some("ugo=rwx".to_owned()),
                                overwrite: true,
                            }],
                            vars: IndexMap::new(),
                        },
                    ]
                }
            };

            let vars_yaml = match self.vars.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "vars:\n  HOME: /home/bob\n".to_owned(),
            };
            let vars = match self.vars.value {
                Some(v) => v,
                None => {
                    let mut vars = IndexMap::new();
                    vars.insert("HOME".to_owned(), "/home/bob".to_owned());
                    vars
                }
            };

            let yaml = format!("{source_yaml}{name_yaml}{hosts_yaml}{include_yaml}{vars_yaml}");
            let manifest = Manifest {
                source,
                name,
                hosts,
                include,
                vars,
            };
            (yaml, manifest)
        }
    }

    /// Blanket "deserialization works" test covering the basic case for all fields.
    #[test]
    fn deserialization_works() {
        Overrides::default().assert_de();
    }

    /// Blanket "serialization works" test covering the basic case for all fields.
    #[test]
    fn serialization_works() {
        Overrides::default().assert_ser();
    }

    mod source {
        use super::*;

        /// Verifies that a serialized Manifest doesn't include a source field.
        #[test]
        fn serialize_skips() {
            Overrides::new()
                .source(Override {
                    yaml: "",
                    value: Some("sketchy-source.yaml"),
                })
                .assert_ser();
        }

        /// Verifies that any source field is ignored during deserialization.
        ///
        /// I would rather that deserialization fail if a skipped field is present, but at a glance,
        /// it doesn't seem like serde provides an easy way to do that. Oh well; it's not critical.
        /// This test documents the behavior and notifies us if it changes.
        #[test]
        fn deserialization_ignores_if_present() {
            Overrides::new()
                .source(Override {
                    yaml: "source: sketchy-file.yaml",
                    value: None,
                })
                .assert_de();
        }
    }

    mod name {
        use super::*;

        /// Verifies that deserialization fails if name is missing from the YAML source.
        ///
        /// This test serves as a proxy for all required fields, verifying that serde and
        /// serde_yaml do indeed require fields by default (even if their types implement Default).
        #[test]
        #[should_panic(expected = "missing field `name`")]
        fn deserialization_fails_if_absent() {
            Overrides::new()
                .name(Override {
                    yaml: "",
                    value: "",
                })
                .assert_de();
        }
    }

    mod hosts {
        use super::*;

        /// Documents the fact that Manifest::hosts deserializes only from a sequence, not from a
        /// single value. It is possible to overcome this, if desired, but doing so requires a more
        /// complicated setup and has thus far been deemed not worth the effort.
        #[test]
        #[should_panic(expected = "expected a sequence")]
        fn deserialization_from_short_form_fails() {
            Overrides::new()
                .hosts(Override {
                    yaml: "hosts: bob",
                    value: vec!["bob"],
                })
                .assert_de();
        }
    }

    // No tests needed for include at this time: everything is covered elsewhere.

    mod vars {
        use super::*;

        /// Verifies that Manifest::vars defaults to an empty collection if vars is missing
        /// from the YAML source.
        #[test]
        fn deserialization_defaults_to_empty_if_absent() {
            Overrides::new()
                .vars(Override {
                    yaml: "",
                    value: IndexMap::new(),
                })
                .assert_de();
        }

        /// Verifies that entries in a deserialized Manifest::vars appear in the same order as
        /// the entries in the YAML source.
        ///
        /// This test is not at all definitive but is meant to promote confidence and give some
        /// indication in case this behavior changes. The nature of serde is such that a lot of
        /// factors can interfere with this ordering.
        #[test]
        fn deserialization_preserves_order() {
            let mut vars = IndexMap::new();
            vars.insert("BETA".to_owned(), "beta".to_owned());
            vars.insert("GAMMA".to_owned(), "gamma".to_owned());
            vars.insert("ALPHA".to_owned(), "alpha".to_owned());

            Overrides::new()
                .vars(Override {
                    yaml: "\
vars:
  BETA: beta
  GAMMA: gamma
  ALPHA: alpha",
                    value: vars,
                })
                .assert_de();
        }

        /// Verifies that vars is omitted during serialization if Manifest::vars is empty.
        #[test]
        fn serialization_skips_if_empty() {
            Overrides::new()
                .vars(Override {
                    yaml: "",
                    value: IndexMap::new(),
                })
                .assert_de();
        }

        /// Verifies that entries in a serialized Manifest::vars appear in the same order as the
        /// entries in the YAML output.
        ///
        /// This test is not at all definitive but is meant to promote confidence and give some
        /// indication in case this behavior changes. The nature of serde is such that a lot of
        /// factors can interfere with this ordering.
        #[test]
        fn serialization_preserves_order() {
            let mut vars = IndexMap::new();
            vars.insert("BETA".to_owned(), "beta".to_owned());
            vars.insert("GAMMA".to_owned(), "gamma".to_owned());
            vars.insert("ALPHA".to_owned(), "alpha".to_owned());

            Overrides::new()
                .vars(Override {
                    yaml: "\
vars:
  BETA: beta
  GAMMA: gamma
  ALPHA: alpha",
                    value: vars,
                })
                .assert_ser();
        }
    }
}

/// This module is almost identical to [manifest], with appropriate alterations for testing
/// [ManifestFile] rather than [Manifest].
mod manifest_file {
    use super::*;

    /// Stores any values you might wish to override from default when creating a YAML-ManifestFile
    /// pair.
    ///
    /// Internally, each field has an outer Option indicating whether it is overridden: a Some
    /// value indicates an override, and None indicates that you wish to use the default value.
    /// String and Option<String> fields in ManifestFiles are converted to `&'static str` for easier
    /// construction of tests.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Overrides {
        source: Override<Option<&'static str>, Option<Option<&'static str>>>,
        name: Override<Option<&'static str>, Option<&'static str>>,
        hosts: Override<Option<&'static str>, Option<Vec<&'static str>>>,
        include: Override<Option<&'static str>, Option<Vec<String>>>,
        vars: Override<Option<&'static str>, Option<IndexMap<String, String>>>,
    }

    impl Overrides {
        /// Returns a default Override with all values set to None.
        fn new() -> Self {
            Self {
                source: Override::default(),
                name: Override::default(),
                hosts: Override::default(),
                include: Override::default(),
                vars: Override::default(),
            }
        }

        setter!(source, Option<&'static str>);
        setter!(name, &'static str);
        setter!(hosts, Vec<&'static str>);
        setter!(include, Vec<String>);
        setter!(vars, IndexMap<String, String>);
    }

    /// Manually generates a correct pairing of a YAML String and a ManifestFile.
    ///
    /// By default (i.e. with a newly minted Overrides value), produces values representing the
    /// most basic test case.
    ///
    /// To override a field, use the methods provided on Overrides.
    fn source_manifest_pair(overrides: Overrides) -> (String, ManifestFile) {
        // Pull an owned String from the user-friendly `&'static str` that Overrides stores. Add a
        // newline if the value is non-empty.
        let source_yaml = match overrides.source.yaml {
            Some(yaml) => match yaml.is_empty() {
                true => yaml.to_owned(),
                false => yaml.to_owned() + "\n",
            },
            None => "".to_owned(),
        };

        // Extract an owned String from Overrides, if applicable.
        let source = match overrides.source.value {
            Some(v) => v.map(str::to_owned),
            None => None,
        };

        // The rest of the values follow the pattern above.

        let name_yaml = match overrides.name.yaml {
            Some(yaml) => match yaml.is_empty() {
                true => yaml.to_owned(),
                false => yaml.to_owned() + "\n",
            },
            None => "name: Console commands\n".to_owned(),
        };
        let name = match overrides.name.value {
            Some(v) => v.to_owned(),
            None => "Console commands".to_owned(),
        };

        let hosts_yaml = match overrides.hosts.yaml {
            Some(yaml) => match yaml.is_empty() {
                true => yaml.to_owned(),
                false => yaml.to_owned() + "\n",
            },
            None => "hosts:\n- bob\n".to_owned(),
        };
        let hosts = match overrides.hosts.value {
            Some(v) => v.into_iter().map(str::to_owned).collect(),
            None => vec!["bob".to_owned()],
        };

        let include_yaml = match overrides.include.yaml {
            Some(yaml) => match yaml.is_empty() {
                true => yaml.to_owned(),
                false => yaml.to_owned() + "\n",
            },
            None => "\
include:
- tasks/console_commands.yaml
- tasks/transfer_config_files.yaml
"
            .to_owned(),
        };
        let include = match overrides.include.value {
            Some(v) => v,
            None => {
                vec![
                    "tasks/console_commands.yaml".to_owned(),
                    "tasks/transfer_config_files.yaml".to_owned(),
                ]
            }
        };

        let vars_yaml = match overrides.vars.yaml {
            Some(yaml) => match yaml.is_empty() {
                true => yaml.to_owned(),
                false => yaml.to_owned() + "\n",
            },
            None => "vars:\n  HOME: /home/bob\n".to_owned(),
        };
        let vars = match overrides.vars.value {
            Some(v) => v,
            None => {
                let mut vars = IndexMap::new();
                vars.insert("HOME".to_owned(), "/home/bob".to_owned());
                vars
            }
        };

        let yaml = format!("{source_yaml}{name_yaml}{hosts_yaml}{include_yaml}{vars_yaml}");
        let manifest = ManifestFile {
            source,
            name,
            hosts,
            include,
            vars,
        };
        (yaml, manifest)
    }

    /// Generates a YAML-ManifestFile value pair using Overrides, deserializes the YAML, and
    /// asserts_eq! on the generated ManifestFile and the deserialized ManifestFile.
    fn assert_de(overrides: Overrides) {
        let (source, manifest) = source_manifest_pair(overrides);
        assert_eq!(
            manifest,
            serde_yaml::from_str::<ManifestFile>(&source).unwrap()
        );
    }

    /// Generates a YAML-ManifestFile value pair using Overrides, serializes the ManifestFile, and
    /// asserts_eq! on the generated YAML and the serialized YAML.
    fn assert_ser(overrides: Overrides) {
        let (source, manifest) = source_manifest_pair(overrides);
        assert_eq!(source, serde_yaml::to_string(&manifest).unwrap());
    }

    /// Blanket "deserialization works" test covering the basic case for all fields.
    #[test]
    fn deserialization_works() {
        assert_de(Overrides::default());
    }

    /// Blanket "serialization works" test covering the basic case for all fields.
    #[test]
    fn serialization_works() {
        assert_ser(Overrides::default());
    }

    mod source {
        use super::*;

        /// Verifies that a serialized ManifestFile doesn't include a source field.
        #[test]
        fn serialize_skips() {
            assert_ser(Overrides::new().source(Override {
                yaml: "",
                value: Some("sketchy-source.yaml"),
            }));
        }

        /// Verifies that any source field is ignored during deserialization.
        ///
        /// I would rather that deserialization fail if a skipped field is present, but at a glance,
        /// it doesn't seem like serde provides an easy way to do that. Oh well; it's not critical.
        /// This test documents the behavior and notifies us if it changes.
        #[test]
        fn deserialization_ignores_if_present() {
            assert_de(Overrides::new().source(Override {
                yaml: "source: sketchy-file.yaml",
                value: None,
            }));
        }
    }

    mod name {
        use super::*;

        /// Verifies that deserialization fails if name is missing from the YAML source.
        ///
        /// This test serves as a proxy for all required fields, verifying that serde and
        /// serde_yaml do indeed require fields by default (even if their types implement Default).
        #[test]
        #[should_panic(expected = "missing field `name`")]
        fn deserialization_fails_if_absent() {
            assert_de(Overrides::new().name(Override {
                yaml: "",
                value: "",
            }));
        }
    }

    mod hosts {
        use super::*;

        /// Documents the fact that ManifestFile::hosts deserializes only from a sequence, not from
        /// a single value. It is possible to overcome this, if desired, but doing so requires a
        /// more complicated setup and has thus far been deemed not worth the effort.
        #[test]
        #[should_panic(expected = "expected a sequence")]
        fn deserialization_from_short_form_fails() {
            assert_de(Overrides::new().hosts(Override {
                yaml: "hosts: bob",
                value: vec!["bob"],
            }));
        }
    }

    // No tests needed for include at this time: everything is covered elsewhere.

    mod vars {
        use super::*;

        /// Verifies that ManifestFile::vars defaults to an empty collection if vars is missing
        /// from the YAML source.
        #[test]
        fn deserialization_defaults_to_empty_if_absent() {
            assert_de(Overrides::new().vars(Override {
                yaml: "",
                value: IndexMap::new(),
            }));
        }

        /// Verifies that entries in a deserialized ManifestFile::vars appear in the same order as
        /// the entries in the YAML source.
        ///
        /// This test is not at all definitive but is meant to promote confidence and give some
        /// indication in case this behavior changes. The nature of serde is such that a lot of
        /// factors can interfere with this ordering.
        #[test]
        fn deserialization_preserves_order() {
            let mut vars = IndexMap::new();
            vars.insert("BETA".to_owned(), "beta".to_owned());
            vars.insert("GAMMA".to_owned(), "gamma".to_owned());
            vars.insert("ALPHA".to_owned(), "alpha".to_owned());

            assert_de(Overrides::new().vars(Override {
                yaml: "\
vars:
  BETA: beta
  GAMMA: gamma
  ALPHA: alpha",
                value: vars,
            }));
        }

        /// Verifies that vars is omitted during serialization if ManifestFile::vars is empty.
        #[test]
        fn serialization_skips_if_empty() {
            assert_de(Overrides::new().vars(Override {
                yaml: "",
                value: IndexMap::new(),
            }));
        }

        /// Verifies that entries in a serialized ManifestFile::vars appear in the same order as the
        /// entries in the YAML output.
        ///
        /// This test is not at all definitive but is meant to promote confidence and give some
        /// indication in case this behavior changes. The nature of serde is such that a lot of
        /// factors can interfere with this ordering.
        #[test]
        fn serialization_preserves_order() {
            let mut vars = IndexMap::new();
            vars.insert("BETA".to_owned(), "beta".to_owned());
            vars.insert("GAMMA".to_owned(), "gamma".to_owned());
            vars.insert("ALPHA".to_owned(), "alpha".to_owned());

            assert_ser(Overrides::new().vars(Override {
                yaml: "\
vars:
  BETA: beta
  GAMMA: gamma
  ALPHA: alpha",
                value: vars,
            }));
        }
    }
}

mod task {
    use super::*;

    /// Stores any values you might wish to override from default when creating a YAML-Task pair.
    ///
    /// Internally, each field has an outer Option indicating whether it is overridden: a Some
    /// value indicates an override, and None indicates that you wish to use the default value.
    /// String and Option<String> fields in Task are converted to `&'static str` for easier
    /// construction of tests.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Overrides {
        source: Override<Option<&'static str>, Option<Option<&'static str>>>,
        name: Override<Option<&'static str>, Option<&'static str>>,
        user: Override<Option<&'static str>, Option<&'static str>>,
        actions: Override<Option<&'static str>, Option<Vec<Action>>>,
        vars: Override<Option<&'static str>, Option<IndexMap<String, String>>>,
    }

    impl Overrides {
        /// Returns a default Override with all values set to None.
        fn new() -> Self {
            Self {
                source: Override::default(),
                name: Override::default(),
                user: Override::default(),
                actions: Override::default(),
                vars: Override::default(),
            }
        }

        setter!(source, Option<&'static str>);
        setter!(name, &'static str);
        setter!(user, &'static str);
        setter!(actions, Vec<Action>);
        setter!(vars, IndexMap<String, String>);

        /// Generates a YAML-Task value pair using Overrides, deserializes the YAML, and asserts_eq!
        /// on the generated Task and the deserialized Task.
        fn assert_de(self) {
            let (source, task) = self.source_task_pair();
            assert_eq!(task, serde_yaml::from_str::<Task>(&source).unwrap());
        }

        /// Generates a YAML-Task value pair using Overrides, serializes the Task, and asserts_eq!
        /// on the generated YAML and the serialized YAML.
        fn assert_ser(self) {
            let (source, task) = self.source_task_pair();
            assert_eq!(source, serde_yaml::to_string(&task).unwrap());
        }

        /// Manually generates a correct pairing of a YAML String and a Task.
        ///
        /// By default (i.e. with a newly minted Overrides value), produces values representing the
        /// most basic test case.
        ///
        /// To override a field, use the methods provided on Overrides.
        fn source_task_pair(self) -> (String, Task) {
            // Pull an owned String from the user-friendly `&'static str` that Overrides stores.
            // Add a newline if the value is non-empty.
            let source_yaml = match self.source.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "".to_owned(),
            };

            // Extract an owned String from Overrides, if applicable.
            let source = match self.source.value {
                Some(v) => v.map(|s| PathBuf::from_str(s).unwrap()),
                None => None,
            };

            // The rest of the values follow the pattern above.

            let name_yaml = match self.name.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "name: Console commands\n".to_owned(),
            };
            let name = match self.name.value {
                Some(v) => v.to_owned(),
                None => "Console commands".to_owned(),
            };

            let user_yaml = match self.user.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "user: bob\n".to_owned(),
            };
            let user = match self.user.value {
                Some(v) => v.to_owned(),
                None => "bob".to_owned(),
            };

            let actions_yaml = match self.actions.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "actions:\n- shell:\n  - pwd\n  - echo hi\n  - uname -r\n".to_owned(),
            };
            let actions = match self.actions.value {
                Some(v) => v,
                None => vec![Action::Shell(vec![
                    "pwd".to_owned(),
                    "echo hi".to_owned(),
                    "uname -r".to_owned(),
                ])],
            };

            let vars_yaml = match self.vars.yaml {
                Some(yaml) => match yaml.is_empty() {
                    true => yaml.to_owned(),
                    false => yaml.to_owned() + "\n",
                },
                None => "vars:\n  HOME: /home/bob\n".to_owned(),
            };
            let vars = match self.vars.value {
                Some(v) => v,
                None => {
                    let mut vars = IndexMap::new();
                    vars.insert("HOME".to_owned(), "/home/bob".to_owned());
                    vars
                }
            };

            let yaml = format!("{source_yaml}{name_yaml}{user_yaml}{actions_yaml}{vars_yaml}");
            let task = Task {
                source,
                name,
                user,
                actions,
                vars,
            };
            (yaml, task)
        }
    }

    /// Blanket "deserialization works" test covering the basic case for all fields.
    #[test]
    fn deserialization_works() {
        Overrides::default().assert_de();
    }

    /// Blanket "serialization works" test covering the basic case for all fields.
    #[test]
    fn serialization_works() {
        Overrides::default().assert_ser();
    }

    mod source {
        use super::*;

        /// Verifies that a serialized Task doesn't include a source field.
        #[test]
        fn serialize_skips() {
            Overrides::new()
                .source(Override {
                    yaml: "",
                    value: Some("sketchy-source.yaml"),
                })
                .assert_ser();
        }

        /// Verifies that any source field is ignored during deserialization.
        ///
        /// I would rather that deserialization fail if a skipped field is present, but at a glance,
        /// it doesn't seem like serde provides an easy way to do that. Oh well; it's not critical.
        /// This test documents the behavior and notifies us if it changes.
        #[test]
        fn deserialization_ignores_if_present() {
            Overrides::new()
                .source(Override {
                    yaml: "source: sketchy-file.yaml",
                    value: None,
                })
                .assert_de();
        }
    }

    mod name {
        use super::*;

        /// Verifies that deserialization fails if name is missing from the YAML source.
        ///
        /// This test serves as a proxy for all required fields, verifying that serde and
        /// serde_yaml do indeed require fields by default (even if their types implement Default).
        #[test]
        #[should_panic(expected = "missing field `name`")]
        fn deserialization_fails_if_absent() {
            Overrides::new()
                .name(Override {
                    yaml: "",
                    value: "",
                })
                .assert_de();
        }
    }

    mod user {
        use super::*;

        /// Verifies that Task::user is initialized to an empty string during deserialization if
        /// user is missing from the YAML source.
        #[test]
        fn deserialization_defaults_to_empty_if_absent() {
            Overrides::new()
                .user(Override {
                    yaml: "",
                    value: "",
                })
                .assert_de();
        }

        /// Verifies that user is omitted during serialization if Task::user is the empty string.
        #[test]
        fn serialization_skips_if_empty() {
            Overrides::new()
                .user(Override {
                    yaml: "",
                    value: "",
                })
                .assert_ser();
        }
    }

    // No tests needed for actions at this time: everything is covered elsewhere.

    mod vars {
        use super::*;

        /// Verifies that Task::vars defaults to an empty collection if vars is missing from the
        /// YAML source.
        #[test]
        fn deserialization_defaults_to_empty_if_absent() {
            Overrides::new()
                .vars(Override {
                    yaml: "",
                    value: IndexMap::new(),
                })
                .assert_de();
        }

        /// Verifies that entries in a deserialized Task::vars appear in the same order as the
        /// entries in the YAML source.
        ///
        /// This test is not at all definitive but is meant to promote confidence and give some
        /// indication in case this behavior changes. The nature of serde is such that a lot of
        /// factors can interfere with this ordering.
        #[test]
        fn deserialization_preserves_order() {
            let mut vars = IndexMap::new();
            vars.insert("BETA".to_owned(), "beta".to_owned());
            vars.insert("GAMMA".to_owned(), "gamma".to_owned());
            vars.insert("ALPHA".to_owned(), "alpha".to_owned());

            Overrides::new()
                .vars(Override {
                    yaml: "\
vars:
  BETA: beta
  GAMMA: gamma
  ALPHA: alpha",
                    value: vars,
                })
                .assert_de();
        }

        /// Verifies that vars is omitted during serialization if Task::vars is empty.
        #[test]
        fn serialization_skips_if_empty() {
            Overrides::new()
                .vars(Override {
                    yaml: "",
                    value: IndexMap::new(),
                })
                .assert_de();
        }

        /// Verifies that entries in a serialized Task::vars appear in the same order as the
        /// entries in the YAML output.
        ///
        /// This test is not at all definitive but is meant to promote confidence and give some
        /// indication in case this behavior changes. The nature of serde is such that a lot of
        /// factors can interfere with this ordering.
        #[test]
        fn serialization_preserves_order() {
            let mut vars = IndexMap::new();
            vars.insert("BETA".to_owned(), "beta".to_owned());
            vars.insert("GAMMA".to_owned(), "gamma".to_owned());
            vars.insert("ALPHA".to_owned(), "alpha".to_owned());

            Overrides::new()
                .vars(Override {
                    yaml: "\
vars:
  BETA: beta
  GAMMA: gamma
  ALPHA: alpha",
                    value: vars,
                })
                .assert_ser();
        }
    }
}
