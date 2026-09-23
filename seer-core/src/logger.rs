//! Seer application logging for CLI streaming and Loki/Grafana.
//!
//! Default is silent. The CLI sets the level with `-v` / `-vv` / `-vvv`.
//! `SEER_LOG_FORMAT=json` switches stderr lines from human-readable to JSON.

use std::fmt;
use std::sync::{Mutex, OnceLock};

static SEER_LOGGER: OnceLock<SeerLogger> = OnceLock::new();
static LOG_LINE_LOCK: Mutex<()> = Mutex::new(());

pub fn init_seer_logger(logger: SeerLogger) {
    let _ = SEER_LOGGER.set(logger);
}

#[inline(always)]
pub fn seer_logger() -> &'static SeerLogger {
    SEER_LOGGER.get().expect("Seer logger not initialized")
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeerLoggerLevel {
    Debug,
    Info,
    Warn,
}

impl SeerLoggerLevel {
    fn as_str(self) -> &'static str {
        match self {
            SeerLoggerLevel::Debug => "debug",
            SeerLoggerLevel::Info => "info",
            SeerLoggerLevel::Warn => "warn",
        }
    }

    fn color_code(self) -> &'static str {
        match self {
            SeerLoggerLevel::Debug => "\x1b[36m",
            SeerLoggerLevel::Info => "\x1b[32m",
            SeerLoggerLevel::Warn => "\x1b[33m",
        }
    }

    fn label_upper(self) -> &'static str {
        match self {
            SeerLoggerLevel::Debug => "DEBUG",
            SeerLoggerLevel::Info => "INFO",
            SeerLoggerLevel::Warn => "WARN",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SeerLogFormat {
    Json,
    Pretty,
}

pub struct SeerLogger {
    level: Option<SeerLoggerLevel>,
    format: SeerLogFormat,
}

#[derive(serde::Serialize)]
struct LogRecord<'a> {
    ts: String,
    level: &'a str,
    target: &'a str,
    message: String,
}

const RESET: &str = "\x1b[0m";

fn format_from_env() -> SeerLogFormat {
    match std::env::var("SEER_LOG_FORMAT")
        .ok()
        .map(|v| v.to_lowercase())
        .as_deref()
    {
        Some("json") => SeerLogFormat::Json,
        _ => SeerLogFormat::Pretty,
    }
}

impl SeerLogger {
    /// `0` silent, `1` warn, `2` info, `3+` debug. Format from `SEER_LOG_FORMAT`.
    pub fn from_verbosity(verbose: u8) -> Self {
        let level = match verbose {
            0 => None,
            1 => Some(SeerLoggerLevel::Warn),
            2 => Some(SeerLoggerLevel::Info),
            _ => Some(SeerLoggerLevel::Debug),
        };
        Self {
            level,
            format: format_from_env(),
        }
    }

    pub fn from_env() -> Self {
        Self::from_verbosity(0)
    }

    #[inline(always)]
    pub fn enabled(&self, msg_level: SeerLoggerLevel) -> bool {
        match self.level {
            None => false,
            Some(min) => msg_level >= min,
        }
    }

    fn emit(&self, level: SeerLoggerLevel, target: &'static str, msg: fmt::Arguments<'_>) {
        if !self.enabled(level) {
            return;
        }

        let message = format!("{msg}");
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let _guard = LOG_LINE_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        match self.format {
            SeerLogFormat::Json => {
                let record = LogRecord {
                    ts,
                    level: level.as_str(),
                    target,
                    message,
                };
                let line = serde_json::to_string(&record).expect("log record serializes to JSON");
                eprintln!("{line}");
            }
            SeerLogFormat::Pretty => {
                eprintln!(
                    "{}[SEER {}]{} {} :: {}",
                    level.color_code(),
                    level.label_upper(),
                    RESET,
                    target,
                    message
                );
            }
        }
    }

    pub fn debug(&self, module: &'static str, msg: fmt::Arguments<'_>) {
        self.emit(SeerLoggerLevel::Debug, module, msg);
    }

    pub fn info(&self, module: &'static str, msg: fmt::Arguments<'_>) {
        self.emit(SeerLoggerLevel::Info, module, msg);
    }

    pub fn warn(&self, module: &'static str, msg: fmt::Arguments<'_>) {
        self.emit(SeerLoggerLevel::Warn, module, msg);
    }
}

#[macro_export]
macro_rules! seer_debug {
    ($($arg:tt)*) => {{
        let logger = $crate::seer_logger();
        if logger.enabled($crate::SeerLoggerLevel::Debug) {
            logger.debug(module_path!(), format_args!($($arg)*));
        }
    }};
}

#[macro_export]
macro_rules! seer_info {
    ($($arg:tt)*) => {{
        let logger = $crate::seer_logger();
        if logger.enabled($crate::SeerLoggerLevel::Info) {
            logger.info(module_path!(), format_args!($($arg)*));
        }
    }};
}

#[macro_export]
macro_rules! seer_warn {
    ($($arg:tt)*) => {{
        let logger = $crate::seer_logger();
        if logger.enabled($crate::SeerLoggerLevel::Warn) {
            logger.warn(module_path!(), format_args!($($arg)*));
        }
    }};
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn verbosity_ladder() {
        let silent = SeerLogger::from_verbosity(0);
        assert!(!silent.enabled(SeerLoggerLevel::Warn));
        assert!(!silent.enabled(SeerLoggerLevel::Info));
        assert!(!silent.enabled(SeerLoggerLevel::Debug));

        let warn = SeerLogger::from_verbosity(1);
        assert!(warn.enabled(SeerLoggerLevel::Warn));
        assert!(!warn.enabled(SeerLoggerLevel::Info));
        assert!(!warn.enabled(SeerLoggerLevel::Debug));

        let info = SeerLogger::from_verbosity(2);
        assert!(info.enabled(SeerLoggerLevel::Warn));
        assert!(info.enabled(SeerLoggerLevel::Info));
        assert!(!info.enabled(SeerLoggerLevel::Debug));

        let debug = SeerLogger::from_verbosity(3);
        assert!(debug.enabled(SeerLoggerLevel::Debug));
        assert!(SeerLogger::from_verbosity(9).enabled(SeerLoggerLevel::Debug));
    }
}
