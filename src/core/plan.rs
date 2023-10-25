//! Types for representing an ordered list of manifests to run.

#[cfg(doc)]
use crate::core::action::Action;
use crate::core::action::HostAction;
use crate::core::manifest::{self, Manifest, TaskIntoIter, TaskIter};
#[cfg(doc)]
use crate::core::task::Task;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

/// A plan of action for executing a given list of manifests.
///
/// This struct constitutes the public interface that executors use to interact with
/// [Manifest]s, [Task]s, and [Action]s on the controller.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The official, ordered list of manifests that comprise the plan.
    ///
    /// Everything else can be computed from these manifests.
    pub manifests: Vec<Manifest>,
}

impl Plan {
    /// Creates an empty [Plan], i.e. one with no [Manifest]s.
    pub fn new() -> Self {
        Plan {
            manifests: Vec::new(),
        }
    }

    /// This function is meant as the easy, default entry point for executors.
    ///
    /// This function doesn't actually know how to parse; it simply calls
    /// [crate::core::manifest::load_manifests] for each file.
    #[allow(unused_variables)]
    pub fn from_manifest_files(files: &[impl AsRef<Path>]) -> anyhow::Result<Self> {
        let mut manifests = vec![];
        for file in files {
            manifests.extend(manifest::load_manifests(file)?);
        }
        Ok(Plan { manifests })
    }

    /// Returns a list of hosts involved in this `Plan` in alphabetical order.
    pub fn hosts(&self) -> Vec<String> {
        let mut set = BTreeSet::new();

        for manifest in &self.manifests {
            for host in &manifest.hosts {
                set.insert(host.to_string());
            }
        }

        set.into_iter().collect()
    }

    /// Returns an execution plan for the specified host.
    ///
    /// Returns [None] if `host` was not in the plan's list of hosts.
    #[allow(unused_variables)]
    pub fn plan_for(&self, host: &str) -> Option<HostPlan> {
        for manifest in &self.manifests {
            // We're intentionally picking the first matching host reference from the plan itself
            // so we can return an internal reference instead of the host value we were passed.
            // It's a minor concern, but it will prevent someone from being surprised that we're
            // holding a reference to the value we passed in when the lifetimes of HostPlan suggest
            // we'll use an internal reference.
            for hst in &manifest.hosts {
                if hst == host {
                    return Some(HostPlan {
                        host: hst,
                        plan: self,
                    });
                }
            }
        }
        None
    }
}

/// A [Plan] in the context of a single host on which it will run.
#[derive(Debug, PartialEq)]
pub struct HostPlan<'p> {
    /// The host on which this plan will run.
    host: &'p str,

    /// The [Plan] to run on the host.
    plan: &'p Plan,
}

impl<'p> HostPlan<'p> {
    /// Returns an iterator over [Action]s on this host.
    pub fn iter(&self) -> HostPlanIter<'p> {
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
#[derive(Debug)]
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
    type Item = Arc<HostAction>;

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

impl<'p> IntoIterator for &HostPlan<'p> {
    type Item = Arc<HostAction>;
    type IntoIter = HostPlanIter<'p>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Owned version of [HostPlanIter].
#[derive(Debug)]
pub struct HostPlanIntoIter {
    /// The host on which the plan is intended to run.
    host: String,

    /// An iterator over the manifests in this plan.
    manifests: std::vec::IntoIter<Manifest>,

    /// The current task iterator, which yields [HostAction] values.
    ///
    /// If there are no manifests in the plan, then there can be no current iterator. Thus,
    /// this must be an optional type.
    current_iter: Option<TaskIntoIter>,
}

impl Iterator for HostPlanIntoIter {
    type Item = Arc<HostAction>;

    fn next(&mut self) -> Option<Self::Item> {
        // Modeled after [HostPlanIter].

        if let Some(ref mut iter) = self.current_iter {
            if let Some(next) = iter.next() {
                return Some(next);
            }
        }

        if let Some(next_manifest) = self.manifests.next() {
            self.current_iter = next_manifest.into_tasks_for(&self.host);
            return self.next();
        }

        None
    }
}

impl<'p> IntoIterator for HostPlan<'p> {
    type Item = Arc<HostAction>;
    type IntoIter = HostPlanIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        // Modeled after [HostPlan::iter].
        HostPlanIntoIter {
            host: self.host.to_string(),
            manifests: self.plan.manifests.clone().into_iter(),
            current_iter: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::plan;
    use super::super::{Action, Manifest, Task};
    use super::*;
    use indexmap::IndexMap;

    mod plan {
        use super::*;

        // from_manifest_files surfaces any errors it encounters, and all the complex work it does
        // is through code that's already under test elsewhere, so we only have to test the happy
        // path.
        mod from_manifest_files {
            use super::*;

            #[test]
            fn works() {
                let manifest1 = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/test/load_manifests/manifest1.yaml");
                let manifest2 = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources/test/load_manifests/manifest2.yaml");
                let manifests = [manifest1, manifest2];
                let manifests = Plan::from_manifest_files(&manifests).unwrap();

                let expected = Plan {
                    manifests: vec![
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
                                    actions: vec![Action::Shell(vec![
                                        "apt install -y $packages".to_owned()
                                    ])],
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
                                    actions: vec![Action::Shell(vec![
                                        "snap install $snaps".to_owned()
                                    ])],
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
                                actions: vec![Action::Shell(vec![
                                    "hostnamectl hostname t470".to_owned()
                                ])],
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
                                actions: vec![Action::Shell(vec![
                                    "hostnamectl hostname zen3".to_owned()
                                ])],
                                vars: IndexMap::new(),
                            }],
                            vars: IndexMap::new(),
                        },
                        Manifest {
                            source: Some(
                                Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("resources/test/load_manifests/manifest2.yaml"),
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
                                actions: vec![Action::Shell(vec![
                                    "hostnamectl hostname t470".to_owned()
                                ])],
                                vars: IndexMap::new(),
                            }],
                            vars: IndexMap::new(),
                        },
                    ],
                };

                assert_eq!(expected, manifests);
            }
        }

        mod hosts {
            use super::*;

            #[test]
            fn works_with_no_manifests() {
                let plan = Plan { manifests: vec![] };
                assert_eq!(Vec::<String>::new(), plan.hosts());
            }

            #[test]
            fn works_with_no_hosts() {
                let (mut plan, _, _, _) = plan();
                plan.manifests[0].hosts.clear();
                assert_eq!(Vec::<String>::new(), plan.hosts());
            }

            #[test]
            fn works_with_multiple_manifests() {
                let (mut plan, _, _, _) = plan();

                // Give `plan` two manifests with no hosts.
                plan.manifests[0].hosts.clear();
                plan.manifests.push(plan.manifests[0].clone());

                // Give the manifests some already-sorted hosts, since we're not testing sorting in
                // this test case.
                plan.manifests[0].hosts.push("aaa".to_string());
                plan.manifests[1].hosts.push("zzz".to_string());

                let expected_hosts = vec!["aaa".to_string(), "zzz".to_string()];
                assert_eq!(expected_hosts, plan.hosts());
            }

            #[test]
            fn deduplicates_hosts() {
                let (mut plan, _, _, _) = plan();

                plan.manifests.push(plan.manifests[0].clone());
                assert_eq!(plan.manifests[0].hosts, plan.hosts());
            }

            #[test]
            fn sorts_hosts() {
                let (mut plan, _, _, _) = plan();

                // Give `plan` two manifests with no hosts.
                plan.manifests[0].hosts.clear();
                plan.manifests.push(plan.manifests[0].clone());

                // Give both manifests out-of-order hosts, so we can make sure they really do get
                // sorted both within a manifest and across manifests.
                plan.manifests[0].hosts.push("zzz".to_string());
                plan.manifests[0].hosts.push("yyy".to_string());
                plan.manifests[1].hosts.push("bbb".to_string());
                plan.manifests[1].hosts.push("aaa".to_string());

                let expected_hosts = vec![
                    "aaa".to_string(),
                    "bbb".to_string(),
                    "yyy".to_string(),
                    "zzz".to_string(),
                ];
                assert_eq!(expected_hosts, plan.hosts());
            }
        }

        mod plan_for {
            use super::*;

            #[test]
            fn works_with_no_manifests() {
                let plan = Plan { manifests: vec![] };
                assert!(plan.plan_for("dave-desktop").is_none());
            }

            #[test]
            fn works_with_no_hosts() {
                let (mut plan, _, _, _) = plan();
                plan.manifests[0].hosts.clear();
                assert!(plan.plan_for("dave-desktop").is_none());
            }

            #[test]
            fn works_with_no_matching_manifests() {
                let (plan, _, _, _) = plan();
                assert!(plan.plan_for("dave-desktop").is_none());
            }

            #[test]
            fn works_with_some_non_matching_manifests() {
                let (_, mut m1, _, _) = plan();
                let mut m2 = m1.clone();
                let mut m3 = m1.clone();

                // m1 and m3 will match; m2 will not and should be skipped.
                m1.hosts = vec!["dave-desktop".into()];
                m2.hosts = vec!["tracy-laptop".into()];
                m3.hosts = vec!["dave-desktop".into()];

                // Name them uniquely so they compare differently with PartialEq.
                m1.name = "m1".into();
                m2.name = "m2".into();
                m3.name = "m3".into();

                let plan = Plan {
                    manifests: vec![m1, m2, m3],
                };

                let expected = HostPlan {
                    host: &plan.manifests[0].hosts[0],
                    plan: &plan,
                };
                assert_eq!(Some(expected), plan.plan_for(&plan.manifests[0].hosts[0]));
            }
        }
    }

    mod iterators {
        // We test both by-reference and by-value iterators for [HostPlan] here. They're parallel,
        // and they both return [HostAction] values, so it makes sense to test them together.

        use super::*;

        #[test]
        fn returns_all_actions_in_a_manifest() {
            let (mut plan, _, _, action) = plan();
            plan.manifests[0].include[0].actions.push(action.clone());

            let host_plan = HostPlan {
                host: &plan.manifests[0].hosts[0],
                plan: &plan,
            };

            #[rustfmt::skip]
            let by_reference: Vec<_> = host_plan
                .iter()
                .map(|ha| ha.action().clone())
                .collect();

            let by_value: Vec<_> = host_plan
                .into_iter()
                .map(|ha| ha.action().clone())
                .collect();

            let expected_host_actions = vec![action.clone(), action];
            assert_eq!(expected_host_actions, by_reference);
            assert_eq!(expected_host_actions, by_value);
        }

        #[test]
        fn iterates_over_multiple_manifests() {
            let (mut plan, manifest, _, action) = plan();
            plan.manifests.push(manifest);

            let host_plan = HostPlan {
                host: &plan.manifests[0].hosts[0],
                plan: &plan,
            };

            #[rustfmt::skip]
            let by_reference: Vec<_> = host_plan
                .iter()
                .map(|ha| ha.action().clone())
                .collect();

            let by_value: Vec<_> = host_plan
                .into_iter()
                .map(|ha| ha.action().clone())
                .collect();

            let expected_host_actions = vec![action.clone(), action];
            assert_eq!(expected_host_actions, by_reference);
            assert_eq!(expected_host_actions, by_value);
        }

        #[test]
        fn skips_empty_manifests() {
            let (mut plan, mut manifest, _, action) = plan();
            manifest.include.clear();
            plan.manifests.insert(0, manifest);

            let host_plan = HostPlan {
                host: &plan.manifests[0].hosts[0],
                plan: &plan,
            };

            #[rustfmt::skip]
            let by_reference: Vec<_> = host_plan
                .iter()
                .map(|ha| ha.action().clone())
                .collect();

            let by_value: Vec<_> = host_plan
                .into_iter()
                .map(|ha| ha.action().clone())
                .collect();

            let expected_host_actions = vec![action];
            assert_eq!(expected_host_actions, by_reference);
            assert_eq!(expected_host_actions, by_value);
        }

        #[test]
        fn skips_manifests_for_different_hosts() {
            let (mut plan, mut manifest, _, action) = plan();
            manifest.hosts.clear();
            plan.manifests.insert(0, manifest);

            let host_plan = HostPlan {
                host: &plan.manifests[1].hosts[0],
                plan: &plan,
            };

            #[rustfmt::skip]
            let by_reference: Vec<_> = host_plan
                .iter()
                .map(|ha| ha.action().clone())
                .collect();

            let by_value: Vec<_> = host_plan
                .into_iter()
                .map(|ha| ha.action().clone())
                .collect();

            let expected_host_actions = vec![action];
            assert_eq!(expected_host_actions, by_reference);
            assert_eq!(expected_host_actions, by_value);
        }

        #[test]
        fn implements_into_iterator() {
            // Just a quick test to verify that we actually implement [IntoIterator] for both
            // by-value and by-reference iteration. The tests above use [IntoIterator] for by-value
            // iteration but use [HostPlan::iter] for by-reference iteration.

            let (plan, _, _, _) = plan();

            let host_plan = HostPlan {
                host: &plan.manifests[0].hosts[0],
                plan: &plan,
            };

            let by_reference: Vec<_> = (&host_plan).into_iter().collect();
            let by_value: Vec<_> = host_plan.into_iter().collect();
            assert_eq!(by_reference, by_value);
        }
    }
}
