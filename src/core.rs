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
