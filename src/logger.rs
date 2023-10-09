//! The public API for building loggers for Sira. Does not contain a logger implementation.
//!
//! If you're looking to implement your own logger, start with [Logger].

#[cfg(doc)]
use crate::executor::Executor;
use crate::executor::Report;
use crate::network;
use crossbeam::channel::{self, Receiver, Sender};

/// A logger for use only in [Executor] that can log [Report]s as well as bare messages.
///
/// If sending fails, this type will failover to standard output and standard error automatically.
#[allow(dead_code)]
pub struct ExecutiveLog {
    reporter: Sender<LogEntry<Report>>,
    raw: Log,
}

impl ExecutiveLog {
    // Returns an ExecutiveLog and the raw Receivers paired with the ExecutiveLog's Senders.
    //
    // This is for use by Executor's tests, so they don't need to inspect the internal states of
    // types in this module that really aren't Executor's concern.
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
#[allow(dead_code)]
pub struct Log {
    raw: Sender<LogEntry<String>>,
}

impl Log {
    /// For testing code that requires a Log, create a [Log] in isolation.
    #[cfg(test)]
    pub fn fixture() -> (Self, Receiver<LogEntry<String>>) {
        let (raw, receiver) = crossbeam::channel::unbounded();
        (Log { raw }, receiver)
    }

    /// Sends a raw, notice-level log message.
    #[allow(unused_variables)]
    pub fn notice(&self, message: String) {
        self.raw.send(LogEntry::Notice(message)).unwrap();
    }

    /// Sends a raw, warning-level log message.
    #[allow(unused_variables)]
    pub fn warning(&self, message: String) {
        todo!();
    }

    /// Sends a raw, error-level log message.
    #[allow(unused_variables)]
    pub fn error(&self, message: String) {
        self.raw.send(LogEntry::Error(message)).unwrap();
    }

    /// Sends a raw, fatal-level log message.
    #[allow(unused_variables)]
    pub fn fatal(&self, message: String) {
        todo!();
    }
}

/// Severity classifications for log entries.
#[derive(Debug)]
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
#[allow(dead_code)]
pub struct LogReceiver<L: Logger> {
    /// Receives [Report] messages from [Executor], already encapsulated as [LogEntry] values.
    reporter: Receiver<LogEntry<Report>>,

    /// Receives raw [LogEntry] messages that can be passed directly to a [Logger].
    raw: Receiver<LogEntry<String>>,

    /// The mechanism for writing logs, e.g. to disk.
    logger: L,
}

impl<L: Logger> LogReceiver<L> {
    #[allow(unused_variables)]
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

    pub fn run(self) {
        // Select between Receivers and dispatch accordingly.
        todo!()
    }
}
