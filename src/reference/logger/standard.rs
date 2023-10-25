//! Writes log files for each client plus one for the controller.

use crate::executor::Report;
use crate::logger::{LogEntry, Logger};
use crate::network;
use anyhow::anyhow;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

/// The name of the log file for log messages that don't pertain to a specific client.
const DEFAULT_LOG_FILE: &str = "sira.log";

/// Opens a log file for appending, or creates it if it did not exist.
fn open_log_file(path: impl AsRef<Path>) -> io::Result<File> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path.as_ref())
}

/// A reference [Logger] that writes one file per client.
///
/// # Panics
///
/// [StandardLogger] makes no effort to handle the intricacies of file system errors.
/// Any method that tries to access the file system panics on any kind of filesystem error.
/// Hoewver, [StandardLogger] is careful to ask for the minimum access that it needs.
#[derive(Debug)]
pub struct StandardLogger {
    /// Maps host names to [ClientLogs] for each host.
    clients: HashMap<String, ClientLogs>,

    /// The directory where logs will be stored.
    directory: PathBuf,

    /// Collects any log messages that don't pertain to a specific client
    default: File,
}

impl StandardLogger {
    /// Create a new [StandardLogger] that stores its files in `directory`.
    ///
    /// Attempts to create `directory` if it does not exist.
    ///
    /// # Returns
    ///
    /// A new [StandardLogger], or any [io::Error] encountered when trying to ensure that
    /// `directory` exists and is a directory.
    pub fn new(directory: impl Into<PathBuf>) -> io::Result<Self> {
        let directory = directory.into();

        match fs::metadata(&directory) {
            Ok(md) if !md.is_dir() => {
                // There's something at this path, but it's not a directory. We can't continue.
                return Err(io::Error::new(
                    ErrorKind::NotFound,
                    anyhow!("not a directory: {}", directory.display()),
                ));
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                // There's nothing at this path. Try to create the directory recursively.
                fs::create_dir_all(&directory)?;
            }
            _ => {}
        }

        let clients = HashMap::new();

        let mut default_log_file_path = directory.clone();
        default_log_file_path.push(DEFAULT_LOG_FILE);
        let default = open_log_file(default_log_file_path)?;

        Ok(Self {
            clients,
            directory,
            default,
        })
    }
}

impl Logger for StandardLogger {
    fn log_raw(&mut self, entry: LogEntry<String>) {
        let entry = entry.to_string().into_bytes();
        self.default.write_all(&entry).unwrap();
    }

    fn log_report(&mut self, mut report: LogEntry<Report>) {
        use network::Report::*;
        use Report::*;

        // Look up the client's host name, if any. If the report doesn't correspond to a specific
        // client, then write to the default log file and return early.
        let host = match report.message() {
            NetworkReport(report) => report.host(),
            Done => {
                let report = report.to_string().into_bytes();
                self.default.write_all(&report).unwrap();
                return;
            }
        };

        // Look up or create the ClientLogs for the Report's host name.
        //
        // Note: if we need to optimize this code, replace the Entry API to get rid of the
        // host.to_string() call. Looking up an existing entry really doesn't need to involve
        // memory allocation, and this is going to happen fairly frequently. However, it probably
        // won't be an issue at the scales for which Sira is designed.
        let client = match self.clients.entry(host.to_string()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(ClientLogs::new(&self.directory, host).unwrap()),
        };

        if let NetworkReport(ActionResult {
            result: Ok(ref mut output),
            ..
        }) = report.message_mut()
        {
            // Divert stdout and stderr to their own files.
            client.stdout.write_all(&output.stdout).unwrap();
            client.stderr.write_all(&output.stderr).unwrap();

            // Clear stdout and sterr from the report to strip them from the event log.
            output.stdout.clear();
            output.stderr.clear();
        }

        let report = report.to_string().into_bytes();
        client.events.write_all(&report).unwrap();
    }
}

/// File handles for log files pertaining to a single client.
#[derive(Debug)]
struct ClientLogs {
    /// [Report]s go here after being stripped of stdout/stderr.
    events: File,

    /// Collects stdout from all commands that run on this client.
    stdout: File,

    /// Collects stderr from all commands that run on this client.
    stderr: File,
}

impl ClientLogs {
    /// Returns a new set of log file handles for `host` inside of `directory`.
    ///
    /// Returns any error encountered while opening/creating log files.
    ///
    /// If `directory` does not exist, it will not be created; log files will simply fail to open.
    fn new(directory: impl Into<PathBuf>, host: impl AsRef<str>) -> io::Result<Self> {
        let mut event_path = directory.into();
        let mut stdout_path = event_path.clone();
        let mut stderr_path = event_path.clone();

        let event_file = format!("{}.log", host.as_ref());
        let stdout_file = format!("{}.stdout.log", host.as_ref());
        let stderr_file = format!("{}.stderr.log", host.as_ref());

        event_path.push(event_file);
        stdout_path.push(stdout_file);
        stderr_path.push(stderr_file);

        let events = open_log_file(event_path)?;
        let stdout = open_log_file(stdout_path)?;
        let stderr = open_log_file(stderr_path)?;
        Ok(Self {
            events,
            stdout,
            stderr,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Action;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    use std::sync::Arc;

    mod fixtures {
        use tempfile::TempDir;

        pub fn tempdir() -> TempDir {
            TempDir::with_prefix("sira-").unwrap()
        }
    }
    use fixtures::*;

    mod open_log_file {
        use super::*;

        #[test]
        fn creates_file() {
            const FILE: &str = "new_file.test";
            let dir = tempdir();
            let file_path = dir.path().join(FILE);

            assert!(fs::metadata(&file_path).is_err());
            open_log_file(&file_path).unwrap();
            assert!(fs::metadata(&file_path).is_ok());
        }

        #[test]
        fn appends_to_file() {
            const FILE: &str = "new_file.test";
            let dir = tempdir();
            let file_path = dir.path().join(FILE);

            // Ensure the file doesn't exist, create it, add some text, and close it.
            assert!(fs::metadata(&file_path).is_err());
            let mut file = open_log_file(&file_path).unwrap();
            assert!(fs::metadata(&file_path).is_ok());
            file.write_all(b"A").unwrap();
            drop(file);

            // Open the file, write something else to it, and close it, giving the code under test
            // the chance to misbehave by either truncating or overwriting.
            let mut file = open_log_file(&file_path).unwrap();
            file.write_all(b"B").unwrap();
            drop(file);

            // Verify that the text from the two writes was appended.
            let contents = fs::read_to_string(&file_path).unwrap();
            assert_eq!("AB", contents);
        }
    }

    mod standard_logger {
        use super::*;

        mod new {
            use super::*;

            #[test]
            fn creates_directory_recursively() {
                // A recursive directory path to create.
                let dir: PathBuf = ["a", "b", "c"].iter().collect();

                let base_dir = tempdir();
                assert!(fs::metadata(base_dir.path().join("a")).is_err());

                let target_dir = base_dir.path().join(dir);
                let _ = StandardLogger::new(&target_dir);

                assert!(fs::metadata(&target_dir).unwrap().is_dir());
            }

            #[test]
            fn creates_default_log_file() {
                let dir = tempdir();

                assert!(fs::metadata(dir.path().join(DEFAULT_LOG_FILE)).is_err());
                let _ = StandardLogger::new(dir.path());
                let file_created = fs::metadata(dir.path().join(DEFAULT_LOG_FILE))
                    .unwrap()
                    .is_file();
                assert!(file_created);
            }

            #[test]
            fn returns_err_if_exists_but_not_directory() {
                let base_dir = tempdir();
                let path = base_dir.path().join("uh-oh");
                let _ = open_log_file(&path).unwrap();

                // The error output in this case is not great, because io_error_more is unstable as
                // of this writing. Therefore, we will be vague in this test.
                assert!(StandardLogger::new(&path).is_err());
            }

            #[test]
            fn returns_err_if_directory_creation_fails() {
                assert!(StandardLogger::new("/sira-test").is_err());
            }

            #[test]
            fn returns_err_if_directory_not_writable() {
                // This test verifies two things:
                // 1. new() returns an error if the directory is not writable
                // 2. new() tries to create the default log file in the directory, which should fail.

                // Create a temporary directory and remove write permission.
                let dir = tempdir();
                let mut perms = fs::metadata(&dir).unwrap().permissions();
                perms.set_readonly(true);
                fs::set_permissions(&dir, perms).unwrap();

                assert_eq!(
                    ErrorKind::PermissionDenied,
                    StandardLogger::new(dir.path()).unwrap_err().kind()
                );
            }
        }

        mod log_raw {
            use super::*;

            #[test]
            fn writes_to_default_log() {
                const EXPECTED: &str = "Oh, no, you got me!";
                let dir = tempdir();
                let mut logger = StandardLogger::new(dir.path()).unwrap();
                logger.log_raw(LogEntry::Fatal(EXPECTED.to_string()));
                drop(logger);
                let default_log = fs::read_to_string(dir.path().join(DEFAULT_LOG_FILE)).unwrap();
                assert!(default_log.contains(EXPECTED));
            }
        }

        mod log_report {
            use super::*;

            #[test]
            fn done_writes_to_default_log_and_returns() {
                let dir = tempdir();

                let mut logger = StandardLogger::new(dir.path()).unwrap();
                logger.log_report(LogEntry::Notice(Report::Done));

                let default_log = fs::read_to_string(dir.path().join(DEFAULT_LOG_FILE)).unwrap();
                assert!(default_log.contains("Done"));

                // Verify that it returns before attempting the next action in the method.
                assert_eq!(0, logger.clients.len());
            }

            #[test]
            fn creates_client_log_files() {
                const HOST: &str = "constantinople";
                let dir = tempdir();

                let client_log = dir.path().join(format!("{HOST}.log"));
                let client_stdout_log = dir.path().join(format!("{HOST}.stdout.log"));
                let client_stderr_log = dir.path().join(format!("{HOST}.stderr.log"));

                // Precondition sanity check.
                assert!(fs::metadata(&client_log).is_err());
                assert!(fs::metadata(&client_stdout_log).is_err());
                assert!(fs::metadata(&client_stderr_log).is_err());

                let mut logger = StandardLogger::new(dir.path()).unwrap();
                let report = LogEntry::Notice(Report::NetworkReport(network::Report::Connecting(
                    HOST.to_string(),
                )));
                logger.log_report(report);
                drop(logger);

                // Sanity check.
                let default_log = fs::read_to_string(dir.path().join(DEFAULT_LOG_FILE)).unwrap();
                assert_eq!(0, default_log.len());

                // Verify test results.
                assert!(fs::metadata(&client_log).unwrap().is_file());
                assert!(fs::metadata(&client_stdout_log).unwrap().is_file());
                assert!(fs::metadata(&client_stderr_log).unwrap().is_file());
            }

            #[test]
            fn diverts_stdout_and_stderr() {
                const HOST: &str = "cairo";
                let dir = tempdir();
                let mut logger = StandardLogger::new(dir.path()).unwrap();
                let report = Report::NetworkReport(network::Report::ActionResult {
                    host: HOST.to_string(),
                    manifest_source: Some("manifest".to_string()),
                    manifest_name: "mname".to_string(),
                    task_source: Some("task".to_string()),
                    task_name: "tname".to_string(),
                    action: Arc::new(Action::Shell(vec!["pwd".to_string()])),
                    result: Ok(Output {
                        status: ExitStatus::from_raw(0),
                        stdout: "Success".into(),
                        stderr: "Error".into(),
                    }),
                });

                let client_log = dir.path().join(format!("{HOST}.log"));
                let client_stdout_log = dir.path().join(format!("{HOST}.stdout.log"));
                let client_stderr_log = dir.path().join(format!("{HOST}.stderr.log"));

                // Run the code under test.
                logger.log_report(LogEntry::Notice(report));

                let client_log = fs::read_to_string(client_log).unwrap();
                let client_stdout_log = fs::read_to_string(client_stdout_log).unwrap();
                let client_stderr_log = fs::read_to_string(client_stderr_log).unwrap();

                assert_eq!("Success", client_stdout_log);
                assert_eq!("Error", client_stderr_log);
                assert!(!client_log.contains("Success"));
                assert!(!client_log.contains("Error"));
            }

            #[test]
            fn writes_reports_to_client_log_file() {
                const HOST: &str = "cairo";
                let dir = tempdir();
                let mut logger = StandardLogger::new(dir.path()).unwrap();
                let report = Report::NetworkReport(network::Report::Connecting(HOST.to_string()));
                let client_log = dir.path().join(format!("{HOST}.log"));

                logger.log_report(LogEntry::Notice(report.clone()));

                let client_log = fs::read_to_string(client_log).unwrap();
                assert!(client_log.contains(&report.to_string()));
            }
        }
    }
}
