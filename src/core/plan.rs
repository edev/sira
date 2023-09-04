//! Types for representing an ordered list of manifests to run.

use crate::core::action::HostAction;
use crate::core::manifest::{Manifest, TaskIter};
use std::path::Path;
use std::sync::Arc;

/// A plan of action for executing a given list of manifests.
///
/// This struct constitutes the public interface that executors use to interact with
/// [Manifest]s, [Task]s, and [Action]s on the controller.
///
/// [Task]: crate::core::task::Task
/// [Action]: crate::core::action::Action
#[derive(Debug, Default)]
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
    #[allow(clippy::result_unit_err)]
    pub fn from_manifest_files(files: &[impl AsRef<Path>]) -> Result<Self, ()> {
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
pub struct HostPlan<'p> {
    /// The host on which this plan will run.
    host: &'p str,

    /// The [Plan] to run on the host.
    plan: &'p Plan,
}

impl<'p> HostPlan<'p> {
    /// Returns an iterator over [Action]s on this host.
    ///
    /// [Action]: crate::core::action::Action
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
