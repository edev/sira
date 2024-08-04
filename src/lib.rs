//! Sira: Simple remote administration.
//!
//! You will find a fairly comprehensive introduction to Sira from an end user's perspective in the
//! [README](https://github.com/edev/sira). Everything in this repository assumes you have already
//! read that document.
//!
//! If you are looking to install and use Sira, check out the [installation
//! guide](https://github.com/edev/sira/blob/main/installation.md).
//!
//! If you want to know more about Sira's security model, take a look at [Securing
//! Sira](https://github.com/edev/sira/blob/main/security.md).
//!
//! If you are a developer working on Sira, take a look at the [developer setup
//! guide](https://github.com/edev/sira/blob/main/developer-setup.md).

pub mod client;
pub mod config;
pub mod core;
pub mod crypto;
pub mod run_plan;

#[doc(inline)]
pub use run_plan::run_plan;
