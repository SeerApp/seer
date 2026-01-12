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

    pub fn trace(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Trace) {
            println!("[SEER TRACE] {:?} {} :: {}", thread::current().id(), module, msg);
        }
    }
    
    pub fn debug(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Debug) {
            println!("[SEER DEBUG] {:?} {} :: {}", thread::current().id(), module, msg);
        }
    }
    
    pub fn warn(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Warn) {
            println!("[SEER WARN] {:?} {} :: {}", thread::current().id(), module, msg);
        }
    }
    
    pub fn error(&self, module: &'static str, msg: fmt::Arguments) {
        if self.enabled(SeerLoggerLevel::Error) {
            eprintln!("[SEER ERROR] {:?} {} :: {}", thread::current().id(), module, msg);
        }
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
