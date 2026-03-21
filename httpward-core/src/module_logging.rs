// httpward-core/src/module_logging.rs
// Module logging utilities for HttpWard dynamic modules
// Provides reusable logging infrastructure for dynamic modules

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Host logging function types with different log levels
pub type HostLogErrorFn = unsafe extern "C" fn(*const c_char);
pub type HostLogWarnFn = unsafe extern "C" fn(*const c_char);
pub type HostLogInfoFn = unsafe extern "C" fn(*const c_char);
pub type HostLogDebugFn = unsafe extern "C" fn(*const c_char);
pub type HostLogTraceFn = unsafe extern "C" fn(*const c_char);

/// Module logging interface that can be used by dynamic modules
pub trait ModuleLogger {
    /// Log error message
    fn error(&self, msg: &str);

    /// Log warning message  
    fn warn(&self, msg: &str);

    /// Log info message
    fn info(&self, msg: &str);

    /// Log debug message
    fn debug(&self, msg: &str);

    /// Log trace message
    fn trace(&self, msg: &str);

    /// Log general message (defaults to info level)
    fn log(&self, msg: &str) {
        self.info(msg);
    }
}

/// Default module logger implementation with host callback support
pub struct DefaultModuleLogger {
    // Host logging callbacks - these will be set by the host
    host_log_error: Option<HostLogErrorFn>,
    host_log_warn: Option<HostLogWarnFn>,
    host_log_info: Option<HostLogInfoFn>,
    host_log_debug: Option<HostLogDebugFn>,
    host_log_trace: Option<HostLogTraceFn>,
    // Module name for log identification
    module_name: String,
}

impl DefaultModuleLogger {
    /// Create a new module logger with default name
    pub const fn new() -> Self {
        Self {
            host_log_error: None,
            host_log_warn: None,
            host_log_info: None,
            host_log_debug: None,
            host_log_trace: None,
            module_name: String::new(),
        }
    }

    /// Create a new module logger with custom name
    pub fn with_name(name: &str) -> Self {
        Self {
            host_log_error: None,
            host_log_warn: None,
            host_log_info: None,
            host_log_debug: None,
            host_log_trace: None,
            module_name: name.to_string(),
        }
    }

    /// Set module name
    pub fn set_module_name(&mut self, name: &str) {
        self.module_name = name.to_string();
    }

    /// Get module name
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    /// Set host logging callbacks
    pub fn set_host_callbacks(
        &mut self,
        error_fn: HostLogErrorFn,
        warn_fn: HostLogWarnFn,
        info_fn: HostLogInfoFn,
        debug_fn: HostLogDebugFn,
        trace_fn: HostLogTraceFn,
    ) {
        self.host_log_error = Some(error_fn);
        self.host_log_warn = Some(warn_fn);
        self.host_log_info = Some(info_fn);
        self.host_log_debug = Some(debug_fn);
        self.host_log_trace = Some(trace_fn);
    }

    /// Get static reference to global logger instance
    pub fn global() -> *mut DefaultModuleLogger {
        static mut GLOBAL_LOGGER: DefaultModuleLogger = DefaultModuleLogger::new();
        &raw mut GLOBAL_LOGGER
    }
}

impl Default for DefaultModuleLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleLogger for DefaultModuleLogger {
    fn error(&self, msg: &str) {
        let prefix = if self.module_name.is_empty() {
            "[module]".to_string()
        } else {
            format!("[{}]", self.module_name)
        };

        if let Some(cb) = self.host_log_error {
            let c = CString::new(format!("{} {}", prefix, msg)).unwrap();
            unsafe { cb(c.as_ptr()) };
        } else {
            // Fallback to direct tracing
            tracing::error!("{} {}", prefix, msg);
        }
    }

    fn warn(&self, msg: &str) {
        let prefix = if self.module_name.is_empty() {
            "[module]".to_string()
        } else {
            format!("[{}]", self.module_name)
        };

        if let Some(cb) = self.host_log_warn {
            let c = CString::new(format!("{} {}", prefix, msg)).unwrap();
            unsafe { cb(c.as_ptr()) };
        } else {
            tracing::warn!("{} {}", prefix, msg);
        }
    }

    fn info(&self, msg: &str) {
        let prefix = if self.module_name.is_empty() {
            "[module]".to_string()
        } else {
            format!("[{}]", self.module_name)
        };

        if let Some(cb) = self.host_log_info {
            let c = CString::new(format!("{} {}", prefix, msg)).unwrap();
            unsafe { cb(c.as_ptr()) };
        } else {
            tracing::info!("{} {}", prefix, msg);
        }
    }

    fn debug(&self, msg: &str) {
        let prefix = if self.module_name.is_empty() {
            "[module]".to_string()
        } else {
            format!("[{}]", self.module_name)
        };

        if let Some(cb) = self.host_log_debug {
            let c = CString::new(format!("{} {}", prefix, msg)).unwrap();
            unsafe { cb(c.as_ptr()) };
        } else {
            tracing::debug!("{} {}", prefix, msg);
        }
    }

    fn trace(&self, msg: &str) {
        let prefix = if self.module_name.is_empty() {
            "[module]".to_string()
        } else {
            format!("[{}]", self.module_name)
        };

        if let Some(cb) = self.host_log_trace {
            let c = CString::new(format!("{} {}", prefix, msg)).unwrap();
            unsafe { cb(c.as_ptr()) };
        } else {
            tracing::trace!("{} {}", prefix, msg);
        }
    }
}

/// Host logging functions that modules can call
pub mod host_functions {
    use super::*;

    /// Host logging function for error level
    #[unsafe(no_mangle)]
    ///
    /// # Safety
    /// `ptr` must be non-null and point to a valid NUL-terminated C string.
    pub unsafe extern "C" fn host_log_error(ptr: *const c_char) {
        let msg = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        tracing::error!(target: "module", "[MODULE] {}", msg);
    }

    /// Host logging function for warn level
    #[unsafe(no_mangle)]
    ///
    /// # Safety
    /// `ptr` must be non-null and point to a valid NUL-terminated C string.
    pub unsafe extern "C" fn host_log_warn(ptr: *const c_char) {
        let msg = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        tracing::warn!(target: "module", "[MODULE] {}", msg);
    }

    /// Host logging function for info level
    #[unsafe(no_mangle)]
    ///
    /// # Safety
    /// `ptr` must be non-null and point to a valid NUL-terminated C string.
    pub unsafe extern "C" fn host_log_info(ptr: *const c_char) {
        let msg = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        tracing::info!(target: "module", "[MODULE] {}", msg);
    }

    /// Host logging function for debug level
    #[unsafe(no_mangle)]
    ///
    /// # Safety
    /// `ptr` must be non-null and point to a valid NUL-terminated C string.
    pub unsafe extern "C" fn host_log_debug(ptr: *const c_char) {
        let msg = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        tracing::debug!(target: "module", "[MODULE] {}", msg);
    }

    /// Host logging function for trace level
    #[unsafe(no_mangle)]
    ///
    /// # Safety
    /// `ptr` must be non-null and point to a valid NUL-terminated C string.
    pub unsafe extern "C" fn host_log_trace(ptr: *const c_char) {
        let msg = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();

        tracing::trace!(target: "module", "[MODULE] {}", msg);
    }
}

/// Module setup utilities
pub mod module_setup {
    use super::*;

    /// Type for module logger setter function
    pub type SetLoggerFn = extern "C" fn(
        HostLogErrorFn,
        HostLogWarnFn,
        HostLogInfoFn,
        HostLogDebugFn,
        HostLogTraceFn,
    );

    /// Setup module logger with host callbacks
    /// This should be called from module_set_logger function
    pub fn setup_module_logger(
        error_fn: HostLogErrorFn,
        warn_fn: HostLogWarnFn,
        info_fn: HostLogInfoFn,
        debug_fn: HostLogDebugFn,
        trace_fn: HostLogTraceFn,
    ) {
        let logger = DefaultModuleLogger::global();
        unsafe {
            (*logger).set_host_callbacks(error_fn, warn_fn, info_fn, debug_fn, trace_fn);
        }
    }

    /// Set module name for the global logger
    pub fn set_module_name(name: &str) {
        let logger = DefaultModuleLogger::global();
        unsafe {
            (*logger).set_module_name(name);
        }
    }

    /// Setup module logger with host callbacks and module name
    /// This should be called from module_set_logger function
    pub fn setup_module_logger_with_name(
        module_name: &str,
        error_fn: HostLogErrorFn,
        warn_fn: HostLogWarnFn,
        info_fn: HostLogInfoFn,
        debug_fn: HostLogDebugFn,
        trace_fn: HostLogTraceFn,
    ) {
        let logger = DefaultModuleLogger::global();
        unsafe {
            (*logger).set_module_name(module_name);
            (*logger).set_host_callbacks(error_fn, warn_fn, info_fn, debug_fn, trace_fn);
        }
    }

    /// Get global module logger instance
    pub fn get_logger() -> &'static DefaultModuleLogger {
        unsafe { &*DefaultModuleLogger::global() }
    }
}

/// Convenience macros for module logging
#[macro_export]
macro_rules! module_log_error {
    ($($arg:tt)*) => {
        $crate::module_logging::module_setup::get_logger().error(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! module_log_warn {
    ($($arg:tt)*) => {
        $crate::module_logging::module_setup::get_logger().warn(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! module_log_info {
    ($($arg:tt)*) => {
        $crate::module_logging::module_setup::get_logger().info(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! module_log_debug {
    ($($arg:tt)*) => {
        $crate::module_logging::module_setup::get_logger().debug(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! module_log_trace {
    ($($arg:tt)*) => {
        $crate::module_logging::module_setup::get_logger().trace(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! module_log {
    ($($arg:tt)*) => {
        $crate::module_logging::module_setup::get_logger().log(&format!($($arg)*))
    };
}
