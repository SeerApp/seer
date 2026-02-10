use std::{fmt, sync::OnceLock, thread};

static SEER_LOGGER: OnceLock<SeerLogger> = OnceLock::new();

pub fn init_seer_logger(logger: SeerLogger) {
    let _ = SEER_LOGGER.set(logger);
}

#[inline(always)]
pub fn seer_logger() -> &'static SeerLogger {
    SEER_LOGGER.get().expect("Seer logger not initialized")
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeerLoggerLevel {
    Trace,
    Debug,
    Warn,
    Error,
}

impl SeerLoggerLevel {
    fn color_code(&self) -> &'static str {
        match self {
            SeerLoggerLevel::Trace => "\x1b[36m", // Cyan
            SeerLoggerLevel::Debug => "\x1b[34m", // Blue
            SeerLoggerLevel::Warn => "\x1b[33m",  // Yellow
            SeerLoggerLevel::Error => "\x1b[31m", // Red
        }
    }

    fn label(&self) -> &'static str {
        match self {
            SeerLoggerLevel::Trace => "TRACE",
            SeerLoggerLevel::Debug => "DEBUG",
            SeerLoggerLevel::Warn => "WARN",
            SeerLoggerLevel::Error => "ERROR",
        }
    }
}

const RESET: &str = "\x1b[0m";

pub struct SeerLogger {
    level: SeerLoggerLevel,
}

impl SeerLogger {
    pub fn from_env() -> Self {
        let level = std::env::var("SEER_LOG")
            .ok()
            .as_deref()
            .map(|v| v.to_lowercase())
            .as_deref()
            .and_then(|v| match v {
                "trace" => Some(SeerLoggerLevel::Trace),
                "debug" => Some(SeerLoggerLevel::Debug),
                "warn"  => Some(SeerLoggerLevel::Warn),
                "error" => Some(SeerLoggerLevel::Error),
                _ => None,
            })
            .unwrap_or(SeerLoggerLevel::Error);

        Self::new(level)
    }

    fn new(level: SeerLoggerLevel) -> Self {
        Self { level }
    }

    #[inline(always)]
    pub fn enabled(&self, msg_level: SeerLoggerLevel) -> bool {
        msg_level >= self.level
    }

    fn log(&self, level: SeerLoggerLevel, module: &'static str, msg: fmt::Arguments) {
        println!(
            "{}[SEER {}]{} {:?} {} :: {}",
            level.color_code(),
            level.label(),
            RESET,
            thread::current().id(),
            module,
            msg
        );
    }

    pub fn trace(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Trace) {
            self.log(SeerLoggerLevel::Trace, module, msg);
        }
    }
    
    pub fn debug(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Debug) {
            self.log(SeerLoggerLevel::Debug, module, msg);
        }
    }
    
    pub fn warn(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Warn) {
            self.log(SeerLoggerLevel::Warn, module, msg);
        }
    }
    
    pub fn error(&self, module: &'static str, msg: fmt::Arguments) {
        eprintln!(
            "{}[SEER ERROR]{} {:?} {} :: {}",
            SeerLoggerLevel::Error.color_code(),
            RESET,
            thread::current().id(),
            module,
            msg
        );
    }    
}

#[macro_export]
macro_rules! seer_trace {
    ($($arg:tt)*) => {{
        let logger = $crate::seer_logger();
        if logger.enabled($crate::SeerLoggerLevel::Trace) {
            logger.trace(module_path!(), format_args!($($arg)*));
        }
    }};
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