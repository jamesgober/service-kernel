//! `proc-daemon`-backed daemon adapter.
//!
//! [`DaemonAdapter`] hosts a [`Kernel`] inside
//! a daemon-managed process. Per `.dev/PLAN.md` §6.8, **one kernel
//! equals one process equals one daemon** — the adapter is single-shot
//! and not designed for multi-daemon scenarios.
//!
//! # Signal handling
//!
//! Both `proc-daemon` and the kernel can install signal handlers.
//! To avoid double-registration this adapter installs the kernel's
//! handler ([`Kernel::install_signal_handler`](crate::kernel::Kernel::install_signal_handler))
//! before running, and does not engage `proc-daemon`'s separate
//! signal layer. Consumers who need `proc-daemon`'s richer signal
//! routing should integrate `proc-daemon` directly rather than
//! through this adapter.
//!
//! # Cross-platform behavior
//!
//! - **Linux / macOS:** the adapter writes a PID file (when
//!   configured) and switches the working directory before the
//!   kernel boots. A future release will add full double-fork
//!   detachment; for `0.1.0` the adapter runs in the foreground.
//! - **Windows:** native service registration is post-`0.1.0`.
//!   Consumers register the binary with `sc.exe` and use this
//!   adapter only for PID file / working-dir hygiene.
//!
//! The current shape gives consumers a forward-compatible API: code
//! written against `DaemonAdapter` today will continue to work when
//! the deeper `proc-daemon` integration lands.

#![cfg(feature = "daemon")]

use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::errors::{KernelError, KernelErrorCode};
use crate::kernel::Kernel;

/// Daemon-time configuration.
///
/// Constructed via [`DaemonConfig::new`] and the chained
/// `with_*`-style builders. Consumers typically pin
/// `pid_file`, `working_dir`, and the user/group identity for an
/// init-managed deployment.
///
/// # Examples
///
/// ```
/// use service_kernel::worker::adapter::DaemonConfig;
///
/// let cfg = DaemonConfig::new("my-service")
///     .pid_file("/var/run/my-service.pid")
///     .working_dir("/var/lib/my-service");
/// assert_eq!(cfg.name, "my-service");
/// assert!(cfg.pid_file.is_some());
/// ```
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Stable daemon name. Used in PID file naming defaults and log
    /// labels.
    pub name: &'static str,
    /// Optional PID file path. Written on `daemonize`, removed on
    /// `run` exit.
    pub pid_file: Option<PathBuf>,
    /// Working directory the daemon switches to before booting the
    /// kernel.
    pub working_dir: Option<PathBuf>,
    /// Optional Unix user to switch to. Reserved for future
    /// double-fork support; ignored on Windows.
    pub user: Option<String>,
    /// Optional Unix group to switch to. Reserved for future
    /// double-fork support; ignored on Windows.
    pub group: Option<String>,
    /// Optional path to redirect stdout to. Reserved for future use.
    pub stdout: Option<PathBuf>,
    /// Optional path to redirect stderr to. Reserved for future use.
    pub stderr: Option<PathBuf>,
}

impl DaemonConfig {
    /// Constructs a default-configured `DaemonConfig` with the given
    /// name.
    #[inline]
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            pid_file: None,
            working_dir: None,
            user: None,
            group: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Sets the PID file path.
    #[must_use]
    pub fn pid_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.pid_file = Some(path.into());
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(path.into());
        self
    }

    /// Sets the target Unix user (reserved for future use).
    #[must_use]
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Sets the target Unix group (reserved for future use).
    #[must_use]
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Sets the stdout redirect path (reserved for future use).
    #[must_use]
    pub fn stdout(mut self, path: impl Into<PathBuf>) -> Self {
        self.stdout = Some(path.into());
        self
    }

    /// Sets the stderr redirect path (reserved for future use).
    #[must_use]
    pub fn stderr(mut self, path: impl Into<PathBuf>) -> Self {
        self.stderr = Some(path.into());
        self
    }
}

impl Default for DaemonConfig {
    /// Returns a `DaemonConfig` with name `""` and no overrides.
    #[inline]
    fn default() -> Self {
        Self::new("")
    }
}

/// `proc-daemon`-backed daemon adapter.
///
/// The adapter owns a [`DaemonConfig`] and, when daemonized, an
/// internal PID-file guard. Use [`DaemonAdapter::new`] to construct,
/// [`DaemonAdapter::daemonize`] to apply daemon-time setup
/// (working-dir change, PID file write), and
/// [`DaemonAdapter::run`] to install signal handling and run the
/// kernel.
///
/// `proc-daemon` is brought in as a feature-gated dependency; the
/// adapter currently exposes the configuration surface and the
/// PID-file / working-dir lifecycle. Deeper `proc-daemon`
/// integration (subsystem registration, hot-reloadable config) is
/// planned for a follow-up release.
pub struct DaemonAdapter {
    config: DaemonConfig,
    pid_file: Mutex<Option<PidFile>>,
}

impl DaemonAdapter {
    /// Constructs an adapter from a [`DaemonConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`KernelError::Subsystem`] if the configuration is
    /// invalid (e.g. an empty `name`).
    pub fn new(config: DaemonConfig) -> Result<Self, KernelError> {
        if config.name.is_empty() {
            return Err(KernelError::Subsystem {
                code: KernelErrorCode::ConfigInvalid,
                name: "daemon",
                source: "daemon name must not be empty".into(),
            });
        }
        Ok(Self {
            config,
            pid_file: Mutex::new(None),
        })
    }

    /// Applies daemon-time setup.
    ///
    /// Today: switches the working directory (if configured) and
    /// writes the PID file (if configured). Future releases will
    /// extend this to perform full double-fork detachment on Unix.
    ///
    /// # Errors
    ///
    /// Returns [`KernelError::Subsystem`] when the working-dir
    /// switch or PID file write fails.
    pub fn daemonize(self) -> Result<Self, KernelError> {
        if let Some(dir) = &self.config.working_dir {
            std::env::set_current_dir(dir).map_err(|e| KernelError::Subsystem {
                code: KernelErrorCode::ConfigInvalid,
                name: "daemon",
                source: format!("working dir {}: {}", dir.display(), e).into(),
            })?;
        }
        if let Some(path) = &self.config.pid_file {
            let pid = std::process::id();
            std::fs::write(path, format!("{}\n", pid)).map_err(|e| {
                KernelError::Subsystem {
                    code: KernelErrorCode::ConfigInvalid,
                    name: "daemon",
                    source: format!("pid file {}: {}", path.display(), e).into(),
                }
            })?;
            let mut guard = self
                .pid_file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = Some(PidFile {
                path: path.clone(),
            });
        }
        Ok(self)
    }

    /// Returns the adapter's configuration.
    #[inline]
    #[must_use]
    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    /// Installs signal handling on `kernel` and runs it.
    ///
    /// On exit, removes the PID file (if one was written by
    /// [`daemonize`](Self::daemonize)). The PID file is the only
    /// shared mutable state the adapter owns; everything else lives
    /// inside the kernel.
    ///
    /// # Errors
    ///
    /// Propagates the [`KernelError`] returned from
    /// [`Kernel::run`](crate::kernel::Kernel::run).
    pub fn run(self, kernel: &Kernel) -> Result<(), KernelError> {
        kernel.install_signal_handler();
        let result = kernel.run();
        // Drop the PID-file guard explicitly so removal happens
        // before this method returns regardless of the kernel's
        // outcome.
        if let Ok(mut guard) = self.pid_file.lock() {
            *guard = None;
        }
        result
    }
}

impl fmt::Debug for DaemonAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonAdapter")
            .field("name", &self.config.name)
            .field("pid_file", &self.config.pid_file)
            .finish_non_exhaustive()
    }
}

/// RAII guard that removes the PID file on drop.
struct PidFile {
    path: PathBuf,
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl fmt::Debug for PidFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PidFile").field("path", &self.path).finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_empty_named() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.name, "");
        assert!(cfg.pid_file.is_none());
        assert!(cfg.working_dir.is_none());
    }

    #[test]
    fn test_builder_chain_sets_each_field() {
        let cfg = DaemonConfig::new("svc")
            .pid_file("/tmp/svc.pid")
            .working_dir("/var/lib/svc")
            .user("svc")
            .group("svc")
            .stdout("/var/log/svc.out")
            .stderr("/var/log/svc.err");
        assert_eq!(cfg.name, "svc");
        assert_eq!(cfg.pid_file.as_deref(), Some(std::path::Path::new("/tmp/svc.pid")));
        assert_eq!(
            cfg.working_dir.as_deref(),
            Some(std::path::Path::new("/var/lib/svc"))
        );
        assert_eq!(cfg.user.as_deref(), Some("svc"));
        assert_eq!(cfg.group.as_deref(), Some("svc"));
        assert_eq!(cfg.stdout.as_deref(), Some(std::path::Path::new("/var/log/svc.out")));
        assert_eq!(cfg.stderr.as_deref(), Some(std::path::Path::new("/var/log/svc.err")));
    }

    #[test]
    fn test_new_rejects_empty_name() {
        let err = DaemonAdapter::new(DaemonConfig::default()).unwrap_err();
        assert!(matches!(err, KernelError::Subsystem { .. }));
    }

    #[test]
    fn test_new_accepts_named_config() {
        let adapter = DaemonAdapter::new(DaemonConfig::new("svc")).unwrap();
        assert_eq!(adapter.config().name, "svc");
    }

    #[test]
    fn test_daemonize_writes_and_drops_pid_file() {
        let tmp = std::env::temp_dir().join(format!(
            "service-kernel-pid-{}.pid",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        let adapter = DaemonAdapter::new(DaemonConfig::new("svc").pid_file(&tmp))
            .unwrap()
            .daemonize()
            .unwrap();
        assert!(tmp.exists());
        let contents = std::fs::read_to_string(&tmp).unwrap();
        assert!(contents.trim().parse::<u32>().is_ok());
        drop(adapter);
        assert!(!tmp.exists());
    }
}
