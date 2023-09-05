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
//! 2. The controller application creates an [executor::Executor], a [ui], a [network], and a
//!    [logger]. These all probably inhabit their own threads, but this decision is left to the
//!    developer of the controller application.
//!
//! 3. The user, either through command-line arguments or UI inputs, provides an ordered list of
//!    [Manifest]s to process. The UI constructs a [Plan] representing this list and passes it to
//!    [Executor].
//!
//! 4. [Executor] compiles a list of all hosts that the [Plan] touches. For each host, [Executor]
//!    instructs the [network] to run the first [Action] for that host.
//!
//! 5. The [network] connects to each host, ideally in parallel, and runs the first action. Typically,
//!    this will mean invoking `sira-client`.
//!
//! 6. Whenever an [Action] is complete, the [network] reports the result to [Executor] and awaits
//!    further instructions for that host. Actions running on other hosts continue independently.
//!
//! 6. Upon receiving a report from the [network] that an [Action] is complete, [Executor]
//!    sends the next [Action] for that host to the [network]. This iteration repeats for
//!    each host until either [Executor] has no more [Action]s to run or the [network]'s
//!    report indicates that execution on the host must abort.
//!
//! [Action]: crate::core::action::Action
//! [Executor]: crate::executor::Executor
//! [Manifest]: crate::core::manifest::Manifest
//! [Plan]: crate::core::plan::Plan

pub mod core;
pub mod executor;
pub mod logger;
pub mod network;
pub mod reference;
pub mod ui;
