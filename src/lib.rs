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
//! 2. The controller application creates a [UI](ui) in [ui::State::Idle], a
//!    [network interface], a [logger], and an [executor::Executor] in
//!    [executor::UiState::Idle]. These all probably inhabit their own threads, but this decision
//!    is left to the developer of the controller application.
//!
//! 3. The user, either through command-line arguments or UI inputs, provides an ordered list of
//!    [Manifest]s to process. The UI constructs a [Plan] representing these [Manifest]s and passes
//!    the [Plan] to [Executor]. This action changes the UI from [ui::State::Idle] to
//!    [ui::State::Plan] and changes [Executor] from [executor::UiState::Idle] to
//!    [executor::UiState::Plan].
//!
//! 4. [Executor] compiles a list of all hosts that the [Plan] touches. For each host, [Executor]
//!    asks the [network interface] to run the first [Action] for that host.
//!
//! 5. The [network interface] connects to each host, ideally in parallel, and runs the first
//!    action. Typically, this will mean invoking `sira-client`. When the [Action] is complete,
//!    the [network interface] reports the result to [Executor] and awaits further instructions.
//!
//! 6. Upon receiving a report from the [network interface] that an [Action] is complete, [Executor]
//!    sends the next [Action] for that host to the [network interface]. This iteration repeats for
//!    each host until either [Executor] has no more [Action]s to run or the [network interface]'s
//!    report indicates that execution on the host must abort.
//!
//! 7. Once the [Plan] is complete, [Executor] sends out an [executor::Message::Idle] message, and
//!    all parties return to their idle states. At that point, if the UI is batch-oriented, it will
//!    most likely exit.
//!
//! [Action]: crate::core::action::Action
//! [Executor]: crate::executor::Executor
//! [Manifest]: crate::core::manifest::Manifest
//! [network interface]: crate::net
//! [Plan]: crate::core::plan::Plan

pub mod core;
pub mod executor;
pub mod logger;
pub mod net;
pub mod ui;
