//! Stage progress feedback (indicatif).
//!
//! - TTY: animated spinner + stage message on stderr
//! - non-TTY / `--quiet`: fully silent (callers keep plain log lines via the
//!   `stage_log!` macro, which degrades to `info!` when progress is hidden)
//!
//! Progress bars are best-effort UI: they never affect program logic.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

static QUIET: AtomicBool = AtomicBool::new(false);

/// Enable/disable progress output (wired to the global `--quiet` flag).
pub fn set_quiet(v: bool) {
    QUIET.store(v, Ordering::Relaxed);
}

/// Whether progress bars are currently shown (quiet off + stderr is a TTY).
pub fn enabled() -> bool {
    !QUIET.load(Ordering::Relaxed) && std::io::stderr().is_terminal()
}

/// A single named stage with an animated spinner.
///
/// Dropping an unfinished stage clears the line; [`Stage::finish`] keeps the
/// final message visible.
pub struct Stage {
    bar: Option<ProgressBar>,
    finished: bool,
}

impl Stage {
    /// Start a stage, e.g. `Stage::new("Packing files")`.
    pub fn new(name: &str) -> Self {
        if !enabled() {
            return Self {
                bar: None,
                finished: false,
            };
        }
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("valid template")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.enable_steady_tick(Duration::from_millis(80));
        bar.set_message(format!("{name}..."));
        Self {
            bar: Some(bar),
            finished: false,
        }
    }

    /// Update the stage message (e.g. current byte count).
    pub fn set_message(&self, msg: impl Into<String>) {
        if let Some(bar) = &self.bar {
            bar.set_message(msg.into());
        }
    }

    /// Mark the stage done, keeping the final message on screen.
    pub fn finish(&mut self, msg: impl Into<String>) {
        if let Some(bar) = &self.bar {
            bar.finish_with_message(msg.into());
        }
        self.finished = true;
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Some(bar) = &self.bar {
            if !self.finished {
                bar.finish_and_clear();
            }
        }
    }
}

/// Stage log helper: use `debug!` when a progress bar carries the feedback,
/// otherwise keep the plain `info!` line (non-TTY "linear log" behaviour).
#[macro_export]
macro_rules! stage_log {
    ($($arg:tt)*) => {
        if $crate::progress::enabled() {
            tracing::debug!($($arg)*);
        } else {
            tracing::info!($($arg)*);
        }
    };
}
