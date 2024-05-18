use super::*;

mod print_host_message {
    use super::*;

    #[test]
    fn works() {
        let mut stdout = vec![];
        print_host_message(&mut stdout, "alice", "Client disconnected").unwrap();
        assert_eq!(
            "[alice] Client disconnected\n",
            String::from_utf8_lossy(&stdout),
        );
    }
}

mod title {
    use super::*;
    use Action::*;

    #[test]
    fn command() {
        assert_eq!("command: ", title(&Command(vec![])));

        assert_eq!(
            "command: foo bar",
            title(&Command(vec!["foo bar".to_string()])),
        );

        assert_eq!(
            "command: foo bar; baz foo; bar baz",
            title(&Command(vec![
                "foo bar".to_string(),
                "baz foo".to_string(),
                "bar baz".to_string(),
            ])),
        );
    }

    #[test]
    fn line_in_file() {
        assert_eq!(
            "line_in_file (/etc/shadow): Mwahahahaha!",
            title(&LineInFile {
                path: "/etc/shadow".to_string(),
                line: "Mwahahahaha!".to_string(),
                pattern: Some("pattern".to_string()),
                after: Some("after".to_string()),
                indent: true,
            }),
        );
    }

    #[test]
    fn script() {
        assert_eq!(
            "script (alice): Set up Alice's user account",
            title(&Script {
                name: "Set up Alice's user account".to_string(),
                user: "alice".to_string(),
                contents: "#!/bin/bash\n\
                    \n\
                    echo Eh, maybe later.\n"
                    .to_string(),
            }),
        );
    }

    #[test]
    fn upload() {
        assert_eq!(
            "upload: from_path -> to_path",
            title(&Upload {
                from: "from_path".to_string(),
                to: "to_path".to_string(),
                user: "alice".to_string(),
                group: "alice".to_string(),
                permissions: Some("644".to_string()),
                overwrite: true,
            }),
        );
    }
}

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
            action: &Action,
            output: Output,
        ) -> (io::Result<()>, Vec<u8>, Vec<u8>) {
            let mut stdout = vec![];
            let mut stderr = vec![];
            let result = _report(&mut stdout, &mut stderr, host.as_ref(), action, &output);
            (result, stdout, stderr)
        }

        // Runs _report() with a fake stdout that will return an error when it tries to write
        // a particular string.
        pub fn test_report_stdout_failure(
            host: impl AsRef<str>,
            action: &Action,
            output: Output,
            failing_line: impl Into<String>,
        ) -> (io::Result<()>, FailingWriter, Vec<u8>) {
            let mut stdout = FailingWriter::new(failing_line.into());
            let mut stderr = vec![];
            let result = _report(&mut stdout, &mut stderr, host.as_ref(), action, &output);
            (result, stdout, stderr)
        }

        // Same as test_report_stdout_failure but for stderr.
        pub fn test_report_stderr_failure(
            host: impl AsRef<str>,
            action: &Action,
            output: Output,
            failing_line: impl Into<String>,
        ) -> (io::Result<()>, Vec<u8>, FailingWriter) {
            let mut stdout = vec![];
            let mut stderr = FailingWriter::new(failing_line.into());
            let result = _report(&mut stdout, &mut stderr, host.as_ref(), action, &output);
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
                #[allow(dead_code)]
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
        let (_, stdout, _) = test_report(
            "bob",
            &Action::Command(vec!["bash -c zsh".to_string()]),
            success(),
        );
        assert!(stdout
            .as_slice()
            .starts_with(b"[bob] Completed command: bash -c zsh"));
    }

    #[test]
    fn returns_error_if_reporting_action_fails() {
        let (result, _, _) = test_report_stdout_failure(
            "bob",
            &Action::Command(vec!["ignore".to_string()]),
            success(),
            "[bob] Completed command: ignore",
        );
        assert!(result.is_err());
    }

    #[test]
    fn reports_stdout() {
        const STDOUT: &str = "please report me";
        let mut output = success();
        output.stdout.extend(STDOUT.bytes());

        let (_, stdout, _) = test_report("", &Action::Command(vec![]), output);

        let stdout = String::from_utf8(stdout).unwrap();
        assert!(stdout.contains("Captured stdout:"));
        assert!(stdout.contains("please report me"));
    }

    #[test]
    fn skips_stdout_if_empty() {
        let (_, stdout, _) = test_report("", &Action::Command(vec![]), success());
        let stdout = String::from_utf8(stdout).unwrap();
        assert!(!stdout.contains("Captured stdout"));
    }

    #[test]
    fn returns_error_if_reporting_stdout_fails() {
        const STDOUT: &str = "out";
        let mut output = success();
        output.stdout.extend(STDOUT.bytes());

        let (result, _, _) =
            test_report_stdout_failure("bob", &Action::Command(vec![]), output, "Captured stdout");
        assert!(result.is_err());
    }

    #[test]
    fn reports_stderr() {
        const STDERR: &str = "please report me";
        let mut output = success();
        output.stderr.extend(STDERR.bytes());

        let (_, _, stderr) = test_report("", &Action::Command(vec![]), output);

        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("Captured stderr:"));
        assert!(stderr.contains("please report me"));
    }

    #[test]
    fn skips_stderr_if_empty() {
        let (_, _, stderr) = test_report("", &Action::Command(vec![]), success());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(!stderr.contains("Captured stderr"));
    }

    #[test]
    fn returns_error_if_reporting_stderr_fails() {
        const STDERR: &str = "err";
        let mut output = success();
        output.stderr.extend(STDERR.bytes());

        let (result, _, _) =
            test_report_stderr_failure("bob", &Action::Command(vec![]), output, "Captured stderr");
        assert!(result.is_err());
    }

    #[test]
    fn reports_error_code_if_any() {
        let command = Action::Command(vec![]);
        let (_, _, stderr) = test_report("bob", &command, error_code(48));
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("[bob] Action failed. See below for details."));
        assert!(stderr.contains("Action exited with exit code 48:"));
        assert!(stderr.contains(&serde_yaml::to_string(&command).unwrap()));
    }

    #[test]
    fn reports_error_message_if_no_error_code() {
        let command = Action::Command(vec![]);
        let (_, _, stderr) = test_report("bob", &command, no_error_code());
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("[bob] Action failed. See below for details."));
        assert!(stderr.contains("Action exited with error:"));
        assert!(stderr.contains(&serde_yaml::to_string(&command).unwrap()));
    }

    #[test]
    fn returns_error_if_reporting_error_code_or_message_fails() {
        let (result, _, _) = test_report_stderr_failure(
            "bob",
            &Action::Command(vec![]),
            no_error_code(),
            "[bob] Action failed.",
        );
        assert!(result.is_err());
    }

    #[test]
    fn returns_ok() {
        let (result, _, _) = test_report("", &Action::Command(vec![]), success());
        assert!(result.is_ok());
    }
}

mod _starting {
    use super::*;

    #[test]
    fn works() {
        let mut stdout: Vec<u8> = Vec::new();
        let action = Action::Command(vec!["bob's your uncle".to_string()]);
        let title = title(&action);
        _starting(&mut stdout, "bob", &action).unwrap();
        assert_eq!(
            format!("[bob] Starting  {title}\n"),
            String::from_utf8_lossy(&stdout),
        );
    }

    #[test]
    fn returns_error_on_failure() {
        struct FailingWriter();
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("failing as expected"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let action = Action::Command(vec!["bob's your uncle".to_string()]);
        assert!(_starting(&mut FailingWriter(), "bob", &action).is_err());
    }
}
