//! This module will contain some sort of interface to a logger. It is NYI.

#[cfg(doc)]
use crate::executor::Executor;
use crate::executor::Report;
use crossbeam::channel::{Receiver, Sender};

/// A logger for use only in [Executor] that can log [Report]s as well as bare messages.
///
/// If sending fails, this type will failover to standard output and standard error automatically.
#[allow(dead_code)]
pub struct ExecutiveLog {
    reporter: Sender<LogEntry<Report>>,
    raw: Log,
}

impl ExecutiveLog {
    /// Logs a [Report] messaage. Automatically classifies it as some [LogEntry] variant based on
    /// message type and contents.
    #[allow(unused_variables)]
    pub fn report(&self, _report: Report) {
        todo!();
    }

    /// Wraps [Log::notice()].
    #[allow(unused_variables)]
    pub fn notice(&self, message: String) {
        self.raw.notice(message);
    }

    /// Wraps [Log::warning()].
    #[allow(unused_variables)]
    pub fn warning(&self, message: String) {
        self.raw.warning(message);
    }

    /// Wraps [Log::error()].
    #[allow(unused_variables)]
    pub fn error(&self, message: String) {
        self.raw.error(message);
    }

    /// Wraps [Log::fatal()].
    #[allow(unused_variables)]
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
    raw: Sender<String>,
}

impl Log {
    /// Sends a raw, notice-level log message.
    #[allow(unused_variables)]
    pub fn notice(&self, message: String) {
        todo!();
    }

    /// Sends a raw, warning-level log message.
    #[allow(unused_variables)]
    pub fn warning(&self, message: String) {
        todo!();
    }

    /// Sends a raw, error-level log message.
    #[allow(unused_variables)]
    pub fn error(&self, message: String) {
        todo!();
    }

    /// Sends a raw, fatal-level log message.
    #[allow(unused_variables)]
    pub fn fatal(&self, message: String) {
        todo!();
    }
}

/// Severity classifications for log entries.
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
        todo!()
    }
}
