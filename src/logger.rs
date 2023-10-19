//! The public API for building loggers for Sira. Does not contain a logger implementation.
//!
//! If you're looking to implement your own logger, start with [Logger].

#[cfg(doc)]
use crate::executor::Executor;
use crate::executor::Report;
use crate::network;
use crossbeam::channel::{self, Receiver, Select, Sender};
use std::io::{self, Write};

/// A logger for use only in [Executor] that can log [Report]s as well as bare messages.
///
/// If sending fails, this type will failover to standard output and standard error automatically.
pub struct ExecutiveLog {
    reporter: Sender<LogEntry<Report>>,
    raw: Log,
}

impl ExecutiveLog {
    // Returns an ExecutiveLog and the raw Receivers paired with the ExecutiveLog's Senders.
    #[cfg(test)]
    pub fn fixture() -> (
        ExecutiveLog,
        Receiver<LogEntry<Report>>,
        Receiver<LogEntry<String>>,
    ) {
        let (reporter, report_recv) = channel::unbounded();
        let (raw, raw_recv) = channel::unbounded();
        let raw = Log { raw };

        let executive_log = ExecutiveLog { reporter, raw };

        (executive_log, report_recv, raw_recv)
    }

    /// Logs a [Report] messaage. Automatically classifies it as some [LogEntry] variant based on
    /// message type and contents.
    pub fn report(&self, report: Report) {
        // TODO Test me.
        use network::Report::*;
        use LogEntry::*;

        match report {
            Report::Done => self.reporter.send(Notice(report)).unwrap(),
            Report::NetworkReport(ref network_report) => match network_report {
                // network::Report implements Display, but we need to classify report before we can
                // stringify and dispatch it.
                FailedToConnect { .. } | Disconnected { .. } => {
                    self.reporter.send(Error(report)).unwrap();
                }
                ActionResult { ref result, .. } => {
                    if result.is_err() {
                        self.reporter.send(Error(report)).unwrap();
                    } else if result.as_ref().unwrap().status.success() {
                        self.reporter.send(Notice(report)).unwrap();
                    } else {
                        // We have a Result::Ok(Output), but Output indicates an error.
                        self.reporter.send(Error(report)).unwrap();
                    }
                }
                _ => self.reporter.send(Notice(report)).unwrap(),
            },
        }
    }

    /// Wraps [Log::notice()].
    pub fn notice(&self, message: String) {
        self.raw.notice(message);
    }

    /// Wraps [Log::warning()].
    pub fn warning(&self, message: String) {
        self.raw.warning(message);
    }

    /// Wraps [Log::error()].
    pub fn error(&self, message: String) {
        self.raw.error(message);
    }

    /// Wraps [Log::fatal()].
    pub fn fatal(&self, message: String) {
        self.raw.fatal(message);
    }
}

/// A logging mechanism for use in other parts of the program.
///
/// Clone one of these and store it in your types that need to send log messages.
///
/// If sending fails, this type will failover to standard output and standard error automatically.
#[derive(Clone)]
pub struct Log {
    raw: Sender<LogEntry<String>>,
}

impl Log {
    /// For testing code that requires a Log. Normally they come as part of a much larger package.
    #[cfg(test)]
    pub fn fixture() -> (Self, Receiver<LogEntry<String>>) {
        let (raw, receiver) = crossbeam::channel::unbounded();
        (Log { raw }, receiver)
    }

    /// Sends a [LogEntry] to [Log::raw]. If that fails, prints it to a backup like stdout/stderr.
    fn raw_send(&self, message: LogEntry<String>, mut backup: impl Write) {
        if self.raw.try_send(message.clone()).is_err() {
            // We don't rely on a Display implementation for LogEntry here, because log entries in
            // a log file need to be a lot more verbose than on-screen messages. We just need very
            // simple, brief, and easy-to-read formatting here.
            let heading = match message {
                LogEntry::Notice(_) => "Notice",
                LogEntry::Warning(_) => "Warning",
                LogEntry::Error(_) => "Error",
                LogEntry::Fatal(_) => "Fatal",
            };
            let message = format!("{heading}: {}\n", message.message());

            // Write to the backup logging mechanism. Deliberately discard any errors.
            let _ = backup.write_all(message.as_bytes());
        }
    }

    /// Sends a raw, notice-level log message.
    pub fn notice(&self, message: String) {
        self._notice(message, io::stdout());
    }

    /// Dependency injection helper for testing.
    fn _notice(&self, message: String, backup: impl Write) {
        self.raw_send(LogEntry::Notice(message), backup);
    }

    /// Sends a raw, warning-level log message.
    pub fn warning(&self, message: String) {
        self._warning(message, io::stderr());
    }

    /// Dependency injection helper for testing.
    fn _warning(&self, message: String, backup: impl Write) {
        self.raw_send(LogEntry::Warning(message), backup);
    }

    /// Sends a raw, error-level log message.
    pub fn error(&self, message: String) {
        self._error(message, io::stderr());
    }

    /// Dependency injection helper for testing.
    fn _error(&self, message: String, backup: impl Write) {
        self.raw_send(LogEntry::Error(message), backup);
    }

    /// Sends a raw, fatal-level log message.
    pub fn fatal(&self, message: String) {
        self._fatal(message, io::stderr());
    }

    /// Dependency injection helper for testing.
    fn _fatal(&self, message: String, backup: impl Write) {
        self.raw_send(LogEntry::Fatal(message), backup);
    }
}

/// Severity classifications for log entries.
#[derive(Clone, Debug, PartialEq)]
pub enum LogEntry<E> {
    /// Just a status update; nothing's wrong.
    Notice(E),

    /// Something minor went wrong, but program execution is continuing.
    Warning(E),

    /// Something significant went wrong, but program execution is continuing.
    Error(E),

    /// Something significant went wrong, and the program is exiting as a result.
    ///
    /// If the user needs to troubleshoot by viewing the log, they are probably looking for this
    /// message at or near the end of the log.
    Fatal(E),
}

impl<E> LogEntry<E> {
    /// Returns the message inside the [LogEntry].
    pub fn message(&self) -> &E {
        use LogEntry::*;
        match self {
            Notice(e) => e,
            Warning(e) => e,
            Error(e) => e,
            Fatal(e) => e,
        }
    }
}

/// An interface to a physical logging mechanism, e.g. a disk logger.
///
/// If you're implementing your own logging system, you simply need to implement this trait on your
/// type.
///
/// Your logger should probably fail over to stdout/stderr if its primary logging (e.g. to disk)
/// fails. This is ultimately up to you as implementer, though.
pub trait Logger {
    /// Write a raw log entry.
    ///
    /// If you receive this, it means some part of the program needed to write a log message
    /// directly rather than sending a [Report] through [Executor]. Pass it directly to your log.
    fn log_raw(&mut self, entry: LogEntry<String>);

    /// Write a log message for a [Report].
    ///
    /// [Report] implements [std::fmt::Display], so you have the option of simply calling
    /// `report.to_string()` if you are satisfied with the default [Report] formatting.
    /// Alternatively, you are free to use a `match` statement and provide your own formatting.
    fn log_report(&mut self, report: LogEntry<Report>);
}

/// Processes log messages from the rest of the program and passes them to a logging mechanism that
/// can write logs, e.g. to disk.
pub struct LogReceiver<L: Logger> {
    /// Receives [Report] messages from [Executor], already encapsulated as [LogEntry] values.
    reporter: Receiver<LogEntry<Report>>,

    /// Receives raw [LogEntry] messages that can be passed directly to a [Logger].
    raw: Receiver<LogEntry<String>>,

    /// The mechanism for writing logs, e.g. to disk.
    logger: L,
}

impl<L: Logger> LogReceiver<L> {
    pub fn new(logger: L) -> (Self, Log, ExecutiveLog) {
        // Construct the base channels we need
        let (reporter, report_recv) = channel::unbounded();
        let (raw, raw_recv) = channel::unbounded();

        let raw = Log { raw };

        let executive_log = ExecutiveLog {
            reporter,
            raw: raw.clone(),
        };

        let receiver = LogReceiver {
            reporter: report_recv,
            raw: raw_recv,
            logger,
        };

        (receiver, raw, executive_log)
    }

    /// Start listening for and processing log messages.
    ///
    /// Blocks until the program is getting ready to exit. You will probably wish to do something
    /// like spawn a thread to run this method.
    pub fn run(mut self) {
        // Select between Receivers and dispatch accordingly. There is no sensible way to order
        // messages that come in concurrently on the different Receivers, so we'll simply allow
        // Select to choose one at random.
        //
        // Note that the channel::select macro, as of this writing, will spam errors if a Receiver
        // closes. Thus, it's not an appropriate choice for the code below. Because this logging
        // mechanism is meant to prioritize resilience, the code below responds intelligently to a
        // closure by removing the Receiver from the list. This also provides an easy way to
        // terminate: close when all Receivers are closed.
        //
        // Also note that the contents of this loop cannot be directly extracted into a method for
        // step-by-step testing with guaranteed termination. The Select value holds immutable
        // references to parts of self, but Logger requires mutable references to self.logger.
        // This code can live here, but it can't be extracted without resolving the conflict,
        // e.g. by changing the definition of Logger to be more restrictive.

        // We could do something fancier, but for just two receivers, manual tracking is simplest.
        let mut reporter = true;
        let mut raw = true;
        let mut select = Select::new();
        let reporter_index = select.recv(&self.reporter);
        let raw_index = select.recv(&self.raw);

        while reporter || raw {
            let operation = select.select();
            match operation.index() {
                i if i == reporter_index => match operation.recv(&self.reporter) {
                    Ok(report) => self.logger.log_report(report),
                    Err(_) => {
                        // The reporter channel is closed. Remove it and try to continue.
                        select.remove(reporter_index);
                        reporter = false;
                    }
                },
                i if i == raw_index => match operation.recv(&self.raw) {
                    Ok(raw) => self.logger.log_raw(raw),
                    Err(_) => {
                        // The raw channel is closed. Remove it and try to continue.
                        select.remove(raw_index);
                        raw = false;
                    }
                },
                i => panic!("LogReceiver received an out-of-bounds select index: {i}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod executive_log {
        use super::*;

        // TODO Write system tests that capture stdout/stderr and verify that notice, warning,
        // error, and fatal print to the correct backup streams. Do the same for those methods in
        // Log as well.

        mod notice {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (executive_log, _report, raw) = ExecutiveLog::fixture();
                executive_log.notice(message.clone());
                assert_eq!(Ok(LogEntry::Notice(message.clone())), raw.try_recv());
            }
        }

        mod warning {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (executive_log, _report, raw) = ExecutiveLog::fixture();
                executive_log.warning(message.clone());
                assert_eq!(Ok(LogEntry::Warning(message.clone())), raw.try_recv());
            }
        }

        mod error {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (executive_log, _report, raw) = ExecutiveLog::fixture();
                executive_log.error(message.clone());
                assert_eq!(Ok(LogEntry::Error(message.clone())), raw.try_recv());
            }
        }

        mod fatal {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (executive_log, _report, raw) = ExecutiveLog::fixture();
                executive_log.fatal(message.clone());
                assert_eq!(Ok(LogEntry::Fatal(message.clone())), raw.try_recv());
            }
        }

        mod report {
            // TODO
        }
    }

    mod log {
        use super::*;

        mod notice {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (log, raw) = Log::fixture();
                log.notice(message.clone());
                assert_eq!(Ok(LogEntry::Notice(message.clone())), raw.try_recv());
            }
        }

        mod warning {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (log, raw) = Log::fixture();
                log.warning(message.clone());
                assert_eq!(Ok(LogEntry::Warning(message.clone())), raw.try_recv());
            }
        }

        mod error {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (log, raw) = Log::fixture();
                log.error(message.clone());
                assert_eq!(Ok(LogEntry::Error(message.clone())), raw.try_recv());
            }
        }

        mod fatal {
            use super::*;

            #[test]
            fn works() {
                let message = "Is this thing on?".to_string();
                let (log, raw) = Log::fixture();
                log.fatal(message.clone());
                assert_eq!(Ok(LogEntry::Fatal(message.clone())), raw.try_recv());
            }
        }

        mod raw_send {
            use super::*;

            // Both ExecutiveLog and Log's public APIs are under test, and those tests cover the
            // happy path through this method. We only need to test failover.

            #[test]
            fn fails_over_to_stderr() {
                let message = "Is this thing on?".to_string();
                let (log, raw) = Log::fixture();
                drop(raw);

                let mut buffer: Vec<u8> = vec![];
                log.raw_send(LogEntry::Notice(message.clone()), &mut buffer);
                log.raw_send(LogEntry::Warning(message.clone()), &mut buffer);
                log.raw_send(LogEntry::Error(message.clone()), &mut buffer);
                log.raw_send(LogEntry::Fatal(message.clone()), &mut buffer);

                let expected = "\
                    Notice: Is this thing on?\n\
                    Warning: Is this thing on?\n\
                    Error: Is this thing on?\n\
                    Fatal: Is this thing on?\n";
                assert_eq!(expected, String::from_utf8(buffer).unwrap());
            }
        }
    }

    mod log_receiver {
        use super::*;
        use std::sync::{Arc, Mutex};
        use std::thread;

        struct TestLogger {
            reports: Vec<LogEntry<Report>>,
            raw: Vec<LogEntry<String>>,
        }

        impl TestLogger {
            fn new() -> Arc<Mutex<Self>> {
                Arc::new(Mutex::new(Self {
                    reports: vec![],
                    raw: vec![],
                }))
            }
        }

        impl Logger for Arc<Mutex<TestLogger>> {
            fn log_raw(&mut self, entry: LogEntry<String>) {
                self.lock().unwrap().raw.push(entry);
            }

            fn log_report(&mut self, report: LogEntry<Report>) {
                self.lock().unwrap().reports.push(report);
            }
        }

        /// Spawns a thread with a LogReceiver<TestLogger>, runs your actions, joins the thread,
        /// and returns the TestLogger for you to inspect.
        ///
        /// Termination is guaranteed unless either LogReceiver::run has a termination logic error
        /// or your closure never terminates.
        fn fixture(actions: impl FnOnce(ExecutiveLog)) -> TestLogger {
            let logger = TestLogger::new();
            let (log_receiver, log, executive_log) = LogReceiver::new(logger.clone());
            let join_handle = thread::spawn(|| log_receiver.run());

            // Drop the Log first so that the ExecutiveLog has the only Sender<Raw>. Thus, if a
            // closure ever needs to test what happens when the last Sender<Raw> closes, that test
            // will actually work as expected. Dropping the Log also allows the LogReceiver::run
            // thread to terminate after the test closure returns.
            drop(log);

            actions(executive_log);
            join_handle.join().unwrap();

            // The caller really doesn't even need to know that we used an Arc<Mutex<_>>.
            Arc::into_inner(logger).unwrap().into_inner().unwrap()
        }

        mod new {
            use super::*;

            #[test]
            fn works() {
                // Verifies that the various return values are wired together correctly.

                let (log_receiver, log, executive_log) = LogReceiver::new(TestLogger::new());

                let raw = LogEntry::Notice("OK".to_string());
                let report = LogEntry::Notice(Report::Done);

                log.raw.try_send(raw.clone()).unwrap();
                executive_log.raw.raw.try_send(raw.clone()).unwrap();
                executive_log.reporter.try_send(report.clone()).unwrap();

                let raw_entries: Vec<_> = log_receiver.raw.try_iter().collect();
                let reports: Vec<_> = log_receiver.reporter.try_iter().collect();

                assert_eq!(vec![raw.clone(), raw], raw_entries);
                assert_eq!(vec![report], reports);
            }
        }

        mod run {
            use super::*;

            // There's no reason to write a test for the termination condition: every test here
            // verifies termination, and we can't really test the reason for termination.

            #[test]
            fn logs_reports() {
                let logger = fixture(|el| el.report(Report::Done));
                assert_eq!(vec![LogEntry::Notice(Report::Done)], logger.reports);
            }

            #[test]
            fn logs_raw_strings() {
                const ENTRY: &str = "Just kidding. Everything's fine!";
                let logger = fixture(|el| el.warning(ENTRY.to_string()));
                assert_eq!(vec![LogEntry::Warning(ENTRY.to_string())], logger.raw);
            }

            #[test]
            fn loops() {
                // For a little extra sanity-checking, we'll send two messages of each type.
                let logger = fixture(|el| {
                    el.report(Report::Done);
                    el.report(Report::Done);
                    el.notice("The cafeteria will close in 15 minutes".to_string());
                    el.notice("The cafeteria will close in 5 minutes".to_string());
                });
                assert_eq!(2, logger.reports.len());
                assert_eq!(2, logger.raw.len());
            }

            #[test]
            fn keeps_logging_if_report_sender_closes() {
                let logger = fixture(|mut el| {
                    el.notice("The cafeteria will close in 15 minutes".to_string());
                    let (s, _) = channel::unbounded();
                    el.reporter = s;
                    el.notice("The cafeteria will close in 5 minutes".to_string());
                });
                assert_eq!(0, logger.reports.len());
                assert_eq!(2, logger.raw.len());
            }

            #[test]
            fn keeps_logging_if_raw_sender_closes() {
                let logger = fixture(|mut el| {
                    el.report(Report::Done);
                    let (s, _) = channel::unbounded();
                    // el.raw is a Log, so we need to replace and drop its Sender.
                    el.raw.raw = s;
                    el.report(Report::Done);
                });
                assert_eq!(2, logger.reports.len());
                assert_eq!(0, logger.raw.len());
            }

            // We could write a test to verify that errors due to channels being closed aren't
            // reported anywhere, but given how complex that test would be and how trivial the
            // code under test is, that code is manually verified.

            // There's no way to test the panic due to out-of-bounds select index, as it should be
            // impossible to reach.
        }
    }
}
