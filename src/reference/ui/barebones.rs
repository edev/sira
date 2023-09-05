//! Contains a barebones text UI.
//!
//! This UI is designed to be simple to write, reasonably obvious, and good for development. It is
//! not necessarily intended to be beautiful or especially user-friendly for production use.

use crate::core::plan::Plan;
use crate::logger::Log;
use crate::ui::{ChannelPair, Message};
use std::env;

/// A seriously barebones textual user interface.
///
/// Accepts a list of manifest file names as command-line arguments, compiles them into a [Plan],
/// hands the [Plan] to Sira, and prints updates as basically an on-screen log.
pub struct BarebonesUi {
    executor: ChannelPair,
    log: Log,
}

impl BarebonesUi {
    /// Creates a ready-to-run [BarebonesUi]. Run it with [BarebonesUi::run()].
    pub fn new(executor: ChannelPair, log: Log) -> Self {
        BarebonesUi { executor, log }
    }

    /// Starts the UI, blocking until the program is getting ready to quit.
    ///
    /// You'll probably want to start this in its own thread.
    pub fn run(self) -> anyhow::Result<()> {
        self._run::<EnvArgs>()
    }

    /// Dependency injection helper for [Self::run].
    fn _run<S: ManifestFileNameSource>(self) -> anyhow::Result<()> {
        let manifest_files = S::manifest_file_names();
        self.notice(format!(
            "Found manifest files: {}",
            manifest_files.join(","),
        ));

        let plan = Plan::from_manifest_files(&manifest_files)?;
        self.notice("Parsed manifest files");

        self.executor.sender.send(Message::RunPlan(plan))?;

        loop {
            use crate::executor::Report::*;
            match self.executor.receiver.recv()? {
                Done => {
                    println!("Done.");
                    break;
                }
                NetworkReport(report) => {
                    println!("{}", report);
                }
            }
        }

        Ok(())
    }

    /// Wrapper around [Self.log.notice] that also prints the message to the screen.
    fn notice<S: Into<String>>(&self, message: S) {
        let message = message.into();
        println!("{}", message);
        self.log.notice(message);
    }
}

/// Dependency injection trait for command-line arguments.
trait ManifestFileNameSource {
    fn manifest_file_names() -> Vec<String>;
}

/// Implements [ManifestFileNameSource] with the real command-line arguments, `env::args()`.
///
/// Skips the first argument, so the result is just the manifest file names.
struct EnvArgs;

impl ManifestFileNameSource for EnvArgs {
    fn manifest_file_names() -> Vec<String> {
        env::args().skip(1).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestArgs;

    impl ManifestFileNameSource for TestArgs {
        fn manifest_file_names() -> Vec<String> {
            vec!["hello".to_string()]
        }
    }
}
