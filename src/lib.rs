//! Simple remote administration.
//!
//! # YAML file types
//!
//! Manifest files and Task files. These are just the names for the file types; you can think of
//! them in whatever terms work for you, e.g. playbooks, roles and profiles, etc.
//!
//! # Program flow
//!
//! This section is meant specifically for developers writing code for Sira. If you are using Sira
//! to manage systems, you don't need to read this section.
//!
//! Below is a high-level overview of Sira's program flow. You can find details of all aspects of
//! the program's flow deeper in the relevant modules' documentation.
//!
//! 1. On the control node, the user invokes Sira via a controller application such as the `sira`
//!    binary.
//!
//! 2. Ahem, this not yet written. Pardon the construction dust.

// TODO Separate large files into submodules, particularly breaking up tests.
// TODO Fill out the top-level documentation above.
// TODO Write an installation guide.
// TODO Create a bootstrapping system for both the control node (if warranted) and managed nodes.
// TODO Create a secure update system.

pub mod client;
pub mod config;
pub mod core;
pub mod crypto;
pub mod run_plan;

#[doc(inline)]
pub use run_plan::run_plan;
