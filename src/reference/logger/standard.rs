//! Contains a basic logger that writes entries to a single file on disk.

use crate::executor::Report;
use crate::logger::{LogEntry, Logger};

struct BasicLogger {}

impl Logger for BasicLogger {
    #[allow(unused_variables)]
    fn log_raw(&mut self, entry: LogEntry<String>) {
        todo!();
    }

    #[allow(unused_variables)]
    fn log_report(&mut self, report: LogEntry<Report>) {
        todo!();
    }
}
