//! Watchdog for a window that stops painting.
//!
//! On Wayland the window could freeze: input still arrived but nothing was
//! drawn again, not even on resize (a frame callback the compositor never
//! sent, see vendor/winit/FERROWL.md). The patched winit now redraws anyway;
//! this watch records in `ui-stall.log` any freeze that still gets through,
//! so it can be told apart from a slow or busy app.
//!
//! It runs inside the event filter and never produces a message, so it costs
//! no update and no redraw.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::{Event, Subscription, mouse, window};

/// A press with no frame after it for this long counts as a freeze.
const STALL_AFTER: Duration = Duration::from_secs(3);

/// Milliseconds since start of the first press not yet followed by a frame
/// (0 = none pending).
static PENDING_PRESS_MS: AtomicU64 = AtomicU64::new(0);
/// Milliseconds since start of the last frame drawn.
static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);
/// The current freeze was already written to the log.
static LOGGED: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    // +1 keeps 0 free to mean "none".
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

pub fn subscription<Message: 'static + Send>() -> Subscription<Message> {
    iced::event::listen_raw(|event, _status, _window| {
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => on_frame(now_ms()),
            Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                if let Some(report) = on_press(now_ms()) {
                    write_log(&report);
                }
            }
            _ => {}
        }
        None
    })
}

fn on_frame(now: u64) {
    LAST_FRAME_MS.store(now, Ordering::Relaxed);
    PENDING_PRESS_MS.store(0, Ordering::Relaxed);
    LOGGED.store(false, Ordering::Relaxed);
}

/// Records a press; returns the log line when an earlier press has gone
/// unanswered for longer than [`STALL_AFTER`].
fn on_press(now: u64) -> Option<String> {
    let pending = PENDING_PRESS_MS.load(Ordering::Relaxed);
    if pending == 0 {
        PENDING_PRESS_MS.store(now, Ordering::Relaxed);
        return None;
    }
    let waited = now.saturating_sub(pending);
    if waited < STALL_AFTER.as_millis() as u64 || LOGGED.swap(true, Ordering::Relaxed) {
        return None;
    }
    let since_frame = now.saturating_sub(LAST_FRAME_MS.load(Ordering::Relaxed));
    Some(format!(
        "{} Ferrowl {}: no frame for {} ms after a click {} ms ago (window not repainting)\n",
        jiff::Zoned::now().strftime("%Y-%m-%dT%H:%M:%S%:z"),
        env!("CARGO_PKG_VERSION"),
        since_frame,
        waited,
    ))
}

fn write_log(line: &str) {
    use std::io::Write;
    let Some(dir) = ferrowl_core::paths::data_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("ui-stall.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The statics are shared, so the whole sequence runs in one test.
    #[test]
    fn logs_once_per_freeze() {
        on_frame(1_000);
        assert!(on_press(2_000).is_none(), "first press only arms the watch");
        assert!(on_press(3_000).is_none(), "too early to call it a freeze");
        let line = on_press(5_500).expect("unanswered for 3.5 s");
        assert!(line.contains("no frame for 4500 ms"));
        assert!(on_press(9_000).is_none(), "same freeze logged once");

        on_frame(9_100);
        assert!(on_press(9_200).is_none(), "a frame ends the freeze");
        assert!(on_press(9_300).is_none());
    }
}
