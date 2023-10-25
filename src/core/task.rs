//! Types for representing task files.

use crate::core::action::Action;
#[cfg(doc)]
use crate::core::manifest::Manifest;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Represents a task file; typically used in the context of a [Manifest].
///
/// This type is typically parsed from a manifest file, but it can be constructed programmatically
/// as well.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Task {
    /// The file from which this value was parsed (if any).
    ///
    /// For instance, a task loaded from a file might set this to the path to the file.
    ///
    /// For task from other sources, e.g. directly from Rust or from network sources,
    /// there is currently no standard value to place here, because these are not intended
    /// use cases for Sira at this time.
    #[serde(skip)]
    pub source: Option<String>,

    /// The [Task]'s name. Used for informational, logging, and debugging purposes.
    pub name: String,

    /// The user on a managed node that should run this [Task]'s [Action]s.
    ///
    /// This is **not** the user Sira will use to log into the host; `sira-client` will switch to
    /// this user to perform actions.
    ///
    /// If this field is empty, then [Self::actions] will run as the login user.
    #[serde(skip_serializing_if = "str::is_empty", default)]
    pub user: String,

    /// The list of [Action]s that comprise this [Task].
    ///
    /// Order is preserved from the source file. Actions are executed in order.
    #[serde(with = "serde_yaml::with::singleton_map_recursive")]
    pub actions: Vec<Action>,

    /// [Task]-level variables, which will eventually be compiled when actions are run.
    ///
    /// Variables are stored as `(name, value)` tuples.
    ///
    /// Order is preserved from the source file but is typically unimportant.
    #[serde(skip_serializing_if = "IndexMap::is_empty", default)]
    pub vars: IndexMap<String, String>,
}
