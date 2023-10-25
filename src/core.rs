//! Provides types that represent the user's instructions, e.g. manifest and task files.

pub mod action;
pub mod manifest;
pub mod plan;
pub mod task;

#[doc(inline)]
pub use action::Action;

#[doc(inline)]
pub use manifest::Manifest;

#[doc(inline)]
pub use plan::Plan;

#[doc(inline)]
pub use task::Task;

#[cfg(test)]
pub mod fixtures {
    use super::*;
    use indexmap::IndexMap;

    /// Returns a [Plan], which contains one [Manifest], which contains one [Task], which contains
    /// one [Action]. Also returns clones of these contained values for convenience.
    pub fn plan() -> (Plan, Manifest, Task, Action) {
        let action = Action::Shell(vec!["echo hi".into(), "pwd".into()]);

        let task = Task {
            source: None,
            name: "API test".into(),
            user: "archie".into(),
            actions: vec![action.clone()],
            vars: IndexMap::new(),
        };

        let manifest = Manifest {
            source: None,
            name: "API test".into(),
            hosts: vec!["archie-desktop".into()],
            include: vec![task.clone()],
            vars: IndexMap::new(),
        };

        let plan = Plan {
            manifests: vec![manifest.clone()],
        };

        (plan, manifest, task, action)
    }
}
