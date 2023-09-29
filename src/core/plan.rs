//! Types for representing an ordered list of manifests to run.

#[cfg(doc)]
use crate::core::action::Action;
use crate::core::action::HostAction;
use crate::core::manifest::{Manifest, TaskIntoIter, TaskIter};
#[cfg(doc)]
use crate::core::task::Task;
use std::path::Path;
use std::sync::Arc;

/// A plan of action for executing a given list of manifests.
///
/// This struct constitutes the public interface that executors use to interact with
/// [Manifest]s, [Task]s, and [Action]s on the controller.
#[derive(Debug, Default, PartialEq)]
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
    ///
    /// The return type should be a Result<Plan, Error>, but I haven't defined the error yet.
    #[allow(unused_variables)]
    pub fn from_manifest_files(files: &[impl AsRef<Path>]) -> anyhow::Result<Self> {
        todo!()
    }

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
    #[allow(unused_variables)]
    pub fn plan_for(&self, host: &str) -> Option<HostPlan> {
        todo!()
    }
}

/// A [Plan] in the context of a single host on which it will run.
#[derive(Debug)]
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
    use super::*;

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

            let host_actions: Vec<_> = host_plan.iter().map(|ha| ha.action().clone()).collect();

            let expected_host_actions = vec![action.clone(), action];
            assert_eq!(expected_host_actions, host_actions);
        }

        #[test]
        fn iterates_over_multiple_manifests() {
            let (mut plan, manifest, _, action) = plan();
            plan.manifests.push(manifest);

            let host_plan = HostPlan {
                host: &plan.manifests[0].hosts[0],
                plan: &plan,
            };

            let host_actions: Vec<_> = host_plan.iter().map(|ha| ha.action().clone()).collect();

            let expected_host_actions = vec![action.clone(), action];
            assert_eq!(expected_host_actions, host_actions);
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

            let host_actions: Vec<_> = host_plan.iter().map(|ha| ha.action().clone()).collect();

            let expected_host_actions = vec![action];
            assert_eq!(expected_host_actions, host_actions);
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

            let host_actions: Vec<_> = host_plan.iter().map(|ha| ha.action().clone()).collect();

            let expected_host_actions = vec![action];
            assert_eq!(expected_host_actions, host_actions);
        }
    }
}
