//! Seer application logging for CLI streaming and Loki/Grafana.
//!
//! Environment:
//! - `SEER_LOG`: `debug`, `info`, `warn`, or `none` — minimum level to emit (`info` if unset).
//!   `none` disables all Seer log output.
//! - `SEER_LOG_FORMAT`: `json` (default) or `pretty` — JSON lines vs colored human output.

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
            SeerLoggerLevel::Debug => "\x1b[36m", // Cyan
            SeerLoggerLevel::Info => "\x1b[32m",  // Green
            SeerLoggerLevel::Warn => "\x1b[33m",  // Yellow
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

impl SeerLogger {
    pub fn from_env() -> Self {
        let level = match std::env::var("SEER_LOG")
            .ok()
            .map(|v| v.to_lowercase())
            .as_deref()
        {
            None => Some(SeerLoggerLevel::Info),
            Some("none") => None,
            Some("debug") => Some(SeerLoggerLevel::Debug),
            Some("info") => Some(SeerLoggerLevel::Info),
            Some("warn") => Some(SeerLoggerLevel::Warn),
            Some(_) => Some(SeerLoggerLevel::Info),
        };

        let format = std::env::var("SEER_LOG_FORMAT")
            .ok()
            .map(|v| v.to_lowercase())
            .as_deref()
            .and_then(|v| match v {
                "pretty" => Some(SeerLogFormat::Pretty),
                "json" => Some(SeerLogFormat::Json),
                _ => None,
            })
            .unwrap_or(SeerLogFormat::Json);

        Self { level, format }
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
                println!("{line}");
            }
            SeerLogFormat::Pretty => {
                println!(
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
