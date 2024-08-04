//! Sira: Simple remote administration.
//!
//! You will find a fairly comprehensive introduction to Sira from an end user's perspective in the
//! [README]. Everything in this repository assumes you have already read that document.
//!
//! If you are looking to install and use Sira, check out the [installation guide].
//!
//! If you want to know more about Sira's security model, take a look at [Securing Sira].
//!
//! If you are a developer working on Sira, take a look at the [developer setup guide].
//!
//! ## Plans, manifests, tasks, and actions
//!
//! The [README] covers [manifests], [tasks], and [actions]. For developers, there's a fourth term
//! to know: a [plan] stores an ordered list of [manifests] and provides facilities for enumerating
//! the [actions] that each host (i.e. managed node) should run. When the user runs the `sira`
//! binary and passes a list of manifest files, that program uses [Plan::from_manifest_files] to
//! parse the files and ultimately return a fully formed [Plan].
//!
//! To execute the [Plan], The `sira` binary then passes it to [run_plan()], which implements the
//! logic documented throughout this crate (e.g. in the [README]). This function sits on its own,
//! rather than being `Plan::run`, because it represents one of many ways to execute a [plan],
//! embedding a wide range of decisions that are documented but ultimately arbitrary. Thus, if you
//! wish to write a function to execute a [plan] differently, e.g. to run each [action] on all
//! hosts in lock-step instead of letting all hosts run through their [actions] as quickly as
//! possible, you aren't competing with a built-in, "official" method on the [Plan] type.
//! Furthermore, if you feel your function might be of use to others, you are free and encouraged
//! to open a pull request to add it to the crate!
//!
//! [README]: https://github.com/edev/sira
//! [installation guide]: https://github.com/edev/sira/blob/main/installation.md
//! [Securing Sira]: https://github.com/edev/sira/blob/main/security.md
//! [developer setup guide]: https://github.com/edev/sira/blob/main/developer-setup.md
//!
//! [plan]: crate::core::Plan
//! [Plan::from_manifest_files]: crate::core::Plan::from_manifest_files
//! [manifests]: crate::core::Manifest
//! [tasks]: crate::core::Task
//! [action]: crate::core::Action
//! [actions]: crate::core::Action
//! [sira]: crate::bin::sira

pub mod client;
pub mod config;
pub mod core;
pub mod crypto;
pub mod run_plan;

#[doc(inline)]
pub use run_plan::run_plan;
