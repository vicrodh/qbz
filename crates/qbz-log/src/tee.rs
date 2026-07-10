//! The composite [`log::Log`] that fans every record to stderr, the ring, and the file.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use log::{Log, Metadata, Record};

use crate::line::LogLine;
use crate::{redact, ring};

/// Wraps `env_logger`'s built `Logger` and tees every record to the in-memory ring and
/// (optionally) the on-disk log file, with secret redaction applied once at this single
/// write choke point. **All** sinks (ring, file, and stderr) receive the redacted text.
pub struct TeeLogger {
    pub(crate) inner: env_logger::Logger,
    pub(crate) file: Option<Mutex<BufWriter<File>>>,
}

fn now_epoch_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

/// Format a redacted log line the same way the file sink does (stable, greppable).
fn format_line(line: &LogLine) -> String {
    format!(
        "{} {:5} {} {}",
        line.format_ts(),
        line.level_str(),
        line.target,
        line.message
    )
}

impl Log for TeeLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        // Honor the inner logger's filter so the ring matches what stderr would show.
        if !self.inner.enabled(record.metadata()) {
            return;
        }

        // Redact ONCE; every downstream sink gets the cleaned text.
        let msg = redact::redact(&record.args().to_string());
        let line = LogLine {
            ts: now_epoch_ms(),
            level: record.level(),
            target: record.target().to_owned(),
            message: msg.clone(),
        };

        ring::push(line.clone());

        if let Some(file) = &self.file {
            if let Ok(mut writer) = file.lock() {
                let _ = writeln!(writer, "{}", format_line(&line));
            }
        }

        // stderr: write the redacted line ourselves. Delegating to
        // `self.inner.log(record)` would reprint the *original* Record args
        // and bypass redaction (terminal transcripts, CI logs, support dumps).
        let _ = writeln!(std::io::stderr(), "{}", format_line(&line));
    }

    fn flush(&self) {
        self.inner.flush();
        if let Some(file) = &self.file {
            if let Ok(mut writer) = file.lock() {
                let _ = writer.flush();
            }
        }
        let _ = std::io::stderr().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level;

    #[test]
    fn stderr_line_is_redacted_before_formatting() {
        redact::register_secret("SEKRET_STDERR_TOKEN".into());

        // Same pipeline as `TeeLogger::log`: redact the raw message once, then
        // format the line that every sink (including stderr) receives.
        let msg = redact::redact("login ok, issued SEKRET_STDERR_TOKEN for session");
        let line = LogLine {
            ts: 0,
            level: Level::Info,
            target: "qbz".into(),
            message: msg,
        };

        let s = format_line(&line);
        assert!(
            !s.contains("SEKRET_STDERR_TOKEN"),
            "secret leaked into stderr line: {s}"
        );
        assert!(s.contains("***REDACTED***"), "no redaction marker: {s}");
    }
}
