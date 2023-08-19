use crate::core::action::Action;

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
