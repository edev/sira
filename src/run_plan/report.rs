//! Reports the outcome of each [Action] that runs on a client.
//!
//! The arrangement of this module is a bit unusual. Instead of presenting a generic method that
//! the user calls with either a real or a fake trait implementation, the user chooses either a
//! real or a fake trait implementation and calls that trait's method, [Report::report]. This trait
//! method calls a private method that provides all of the logic to report the outcome of an
//! [Action] to writers that can be either real or fake stdout and stderr writers. This odd
//! arrangement allows the real trait implementation to lock stdout and stderr just before
//! reporting and release the locks as soon as reporting is done. Locking this way allows most of
//! the program to write to stdout and stderr freely but prevents the output of multiple hosts
//! from getting mangled if they try to report at the same time.
//!
//! [Action]: crate::core::Action

use crate::core::Action;
use async_trait::async_trait;
use std::io::{self, Write};
use std::ops::DerefMut;
use std::process::Output;
use tokio::task;

/// Prints feedback about each [Action] run on a client to stdout/stderr to keep the user informed.
///
/// [Action]: crate::core::Action
#[async_trait]
pub trait Report {
    /// Reports that an action is about to commence.
    fn starting(&mut self, host: &str, action: &Action);

    /// Reports the outcome of an action.
    async fn report(&mut self, host: &str, yaml: String, output: &Output) -> io::Result<()>;
}

/// The real, production-ready [Report] implementation. Uses the real stdout/stderr.
#[derive(Clone, Debug)]
pub struct Reporter;

#[async_trait]
impl Report for Reporter {
    fn starting(&mut self, host: &str, action: &Action) {
        // This trait method is an experimental addition. The implementation below might provide a
        // good basis for a rewrite of Reporter::report(), as well. There are refactoring
        // opportunities in the code below, and that's intentional. At the current stage, this code
        // is meant as a lightly hand-verified design prototype. In time, a more substantial UI
        // rewrite can incorporate lessons learned.
        use Action::*;
        let action = match action {
            Command(vec) => {
                // It's unlikely that vec has more than one element, but that's not our concern.
                format!("command: {}", vec.join(";"))
            }
            LineInFile { path, .. } => format!("line_in_file: {path}"),
            Script { name, .. } => format!("script: {name}"),
            Upload { from, to, .. } => format!("upload: {from} -> {to}"),
        };
        println!("[{host}] Starting {action}");
    }

    async fn report(&mut self, host: &str, yaml: String, output: &Output) -> io::Result<()> {
        // Lock stdout and stderr for sane output ordering. For this same reason, we do not use
        // Tokio's async IO, which provides no locking mechanisms.
        //
        // We need to release the locks as soon as we're done reporting rather than holding them
        // across invocations, so we construct them here instead of storing them in the struct.
        let mut stdout = io::stdout().lock();
        let mut stderr = io::stderr().lock();
        task::block_in_place(move || _report(&mut stdout, &mut stderr, host, yaml, output))
    }
}

/// A testable method containing the logic for reporting the outcome of an [Action].
///
/// [Action]: crate::core::Action
pub fn _report<OT: Write, ET: Write, O: DerefMut<Target = OT>, E: DerefMut<Target = ET>>(
    mut stdout: O,
    mut stderr: E,
    host: &str,
    yaml: String,
    output: &Output,
) -> io::Result<()> {
    writeln!(stdout, "Ran action on {host}:\n{yaml}")?;

    if !output.stdout.is_empty() {
        writeln!(
            stdout,
            "Captured stdout:\n{}",
            String::from_utf8_lossy(&output.stdout),
        )?;
    }

    if !output.stderr.is_empty() {
        writeln!(
            stderr,
            "Captured stderr:\n{}",
            String::from_utf8_lossy(&output.stderr),
        )?;
    }

    if !output.status.success() {
        let exit_code_message = match output.status.code() {
            Some(i) => format!("exit code {i}"),
            None => "error".to_string(),
        };
        writeln!(stderr, "action exited with {exit_code_message}:\n{yaml}")?;
    }

    // Put a space before the next command's report, since this one succeeded.
    writeln!(stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod _report {
        use super::*;
        use std::os::unix::process::ExitStatusExt;
        use std::process::{Command, ExitStatus};

        pub mod fixtures {
            use super::*;

            // Runs _report() with specified options and fake stdout/stderr. Returns values for
            // tests to examine afterward.
            pub fn test_report(
                host: impl AsRef<str>,
                yaml: impl Into<String>,
                output: Output,
            ) -> (io::Result<()>, Vec<u8>, Vec<u8>) {
                let mut stdout = vec![];
                let mut stderr = vec![];

                let result = _report(
                    &mut stdout,
                    &mut stderr,
                    host.as_ref(),
                    yaml.into(),
                    &output,
                );

                (result, stdout, stderr)
            }

            // Runs _report() with a fake stdout that will return an error when it tries to write
            // a particular string.
            pub fn test_report_stdout_failure(
                host: impl AsRef<str>,
                yaml: impl Into<String>,
                output: Output,
                failing_line: impl Into<String>,
            ) -> (io::Result<()>, FailingWriter, Vec<u8>) {
                let mut stdout = FailingWriter::new(failing_line.into());
                let mut stderr = vec![];

                let result = _report(
                    &mut stdout,
                    &mut stderr,
                    host.as_ref(),
                    yaml.into(),
                    &output,
                );

                (result, stdout, stderr)
            }

            // Same as test_report_stdout_failure but for stderr.
            pub fn test_report_stderr_failure(
                host: impl AsRef<str>,
                yaml: impl Into<String>,
                output: Output,
                failing_line: impl Into<String>,
            ) -> (io::Result<()>, Vec<u8>, FailingWriter) {
                let mut stdout = vec![];
                let mut stderr = FailingWriter::new(failing_line.into());

                let result = _report(
                    &mut stdout,
                    &mut stderr,
                    host.as_ref(),
                    yaml.into(),
                    &output,
                );

                (result, stdout, stderr)
            }

            // Returns an Output value representing a blank, successful program return.
            pub fn success() -> Output {
                Output {
                    status: ExitStatus::from_raw(0),
                    stdout: vec![],
                    stderr: vec![],
                }
            }

            // Returns an Output value with a custom exit code.
            pub fn error_code(code: i32) -> Output {
                // The only I know to construct an ExitStatus value whose code() method returns
                // a Some value is to actually run a child process. The following assertion
                // illustrates the problem:
                assert!(ExitStatus::from_raw(code).code().is_none());

                let output = Command::new("/bin/sh")
                    .arg("-c")
                    .arg(format!("return {code}"))
                    .output()
                    .unwrap();

                // Sanity checks, since we're doing something janky.
                assert_eq!(Some(code), output.status.code());
                assert!(output.stdout.is_empty());
                assert!(output.stderr.is_empty());
                output
            }

            // Returns an output value that represents an error exit with no available exit code.
            pub fn no_error_code() -> Output {
                Output {
                    // This counts as a unix_wait_status code, not an exit code, so `output.code()`
                    // evaluates to None.
                    status: ExitStatus::from_raw(1),
                    stdout: vec![],
                    stderr: vec![],
                }
            }

            pub mod failing_writer {
                use super::*;

                // A fake stdout/stderr that "fails" when asked to write a specific line chosen by
                // the user. The test fails if FailingWriter never receives the expected line.
                #[derive(Debug)]
                pub struct FailingWriter {
                    // If this is a Some value when the struct is dropped, the test fails.
                    failing_line: Option<String>,

                    // Records anything successfully "written" here.
                    buffer: String,
                }

                impl FailingWriter {
                    pub fn new(failing_line: String) -> Self {
                        Self {
                            failing_line: Some(failing_line),
                            buffer: String::new(),
                        }
                    }

                    // Returns everything written to the writer so far.
                    pub fn buffer(&self) -> &str {
                        &self.buffer
                    }
                }

                impl Write for FailingWriter {
                    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                        let input = String::from_utf8(buf.to_vec())
                            .expect("FailingWriter only accept valid UTF-8 inputs");
                        let line = match self.failing_line {
                            Some(ref line) => line,
                            None => panic!("tried to write after a failed write"),
                        };

                        // Because write! and similar can make lots of small calls to write, we
                        // can't assume that buf contains the whole line that the user wants to
                        // write. It probably doesn't. Thus, we write the contents to the buffer,
                        // and then we fail if the buffer contains self.failing_line.
                        self.buffer.push_str(&input);
                        if self.buffer.contains(line) {
                            // We've found our expected failure. Clear it so we know not to fail
                            // the test, and then return the expected error.
                            self.failing_line = None;
                            Err(io::Error::other("failing as expected"))
                        } else {
                            Ok(buf.len())
                        }
                    }

                    fn flush(&mut self) -> io::Result<()> {
                        Ok(())
                    }
                }

                impl Drop for FailingWriter {
                    fn drop(&mut self) {
                        // If we never saw the expected line, the test presumably has a bug.
                        if !std::thread::panicking() && self.failing_line.is_some() {
                            panic!(
                                "never received failing line: {}",
                                self.failing_line.as_ref().unwrap()
                            );
                        }
                    }
                }
            }
            pub use failing_writer::*;
        }
        use fixtures::*;

        #[test]
        fn reports_action() {
            let (_, stdout, _) = test_report("bob", "some_yaml", success());
            assert!(stdout
                .as_slice()
                .starts_with(b"Ran action on bob:\nsome_yaml"));
        }

        #[test]
        fn returns_error_if_reporting_action_fails() {
            let (result, _, _) =
                test_report_stdout_failure("bob", "some_yaml", success(), "Ran action on bob");
            assert!(result.is_err());
        }

        #[test]
        fn reports_stdout() {
            const STDOUT: &str = "out";
            let mut output = success();
            output.stdout.extend(STDOUT.bytes());

            let (_, stdout, _) = test_report("", "", output);

            let stdout = String::from_utf8(stdout).unwrap();
            assert!(stdout.contains("Captured stdout:\nout"));
        }

        #[test]
        fn skips_stdout_if_empty() {
            let (_, stdout, _) = test_report("", "", success());
            let stdout = String::from_utf8(stdout).unwrap();
            assert!(!stdout.contains("Captured stdout"));
        }

        #[test]
        fn returns_error_if_reporting_stdout_fails() {
            const STDOUT: &str = "out";
            let mut output = success();
            output.stdout.extend(STDOUT.bytes());

            let (result, _, _) =
                test_report_stdout_failure("bob", "some_yaml", output, "Captured stdout");
            assert!(result.is_err());
        }

        #[test]
        fn reports_stderr() {
            const STDERR: &str = "err";
            let mut output = success();
            output.stderr.extend(STDERR.bytes());

            let (_, _, stderr) = test_report("", "", output);

            let stderr = String::from_utf8(stderr).unwrap();
            assert!(stderr.contains("Captured stderr:\nerr"));
        }

        #[test]
        fn skips_stderr_if_empty() {
            let (_, _, stderr) = test_report("", "", success());
            let stderr = String::from_utf8(stderr).unwrap();
            assert!(!stderr.contains("Captured stderr"));
        }

        #[test]
        fn returns_error_if_reporting_stderr_fails() {
            const STDERR: &str = "err";
            let mut output = success();
            output.stderr.extend(STDERR.bytes());

            let (result, _, _) =
                test_report_stderr_failure("bob", "some_yaml", output, "Captured stderr");
            assert!(result.is_err());
        }

        #[test]
        fn writes_trailing_newline() {
            // yaml must be non-empty, otherwise the \n before yaml will incorrectly count as a
            // trailing newline.
            let (_, stdout, _) = test_report("", "yaml", success());
            assert!(stdout.as_slice().ends_with(b"\n\n"));
        }

        #[test]
        fn returns_error_if_writing_trailing_newline_fails() {
            let (result, stdout, _) =
                test_report_stdout_failure("bob", "some_yaml", success(), "\n\n");

            // Make sure we actually got through the rest of the code.
            assert!(stdout.buffer().contains("some_yaml"));

            // Make sure we failed properly.
            assert!(result.is_err());
        }

        #[test]
        fn reports_error_code_if_any() {
            let (_, _, stderr) = test_report("", "llama", error_code(48));
            assert!(stderr.ends_with(b"action exited with exit code 48:\nllama\n"));
        }

        #[test]
        fn reports_error_message_if_no_error_code() {
            let (_, _, stderr) = test_report("", "llama", no_error_code());
            assert!(stderr.ends_with(b"action exited with error:\nllama\n"));
        }

        #[test]
        fn returns_error_if_reporting_error_code_or_message_fails() {
            let (result, _, _) =
                test_report_stderr_failure("bob", "llama", no_error_code(), "action exited with");
            assert!(result.is_err());
        }

        #[test]
        fn returns_ok() {
            let (result, _, _) = test_report("", "", success());
            assert!(result.is_ok());
        }
    }
}
