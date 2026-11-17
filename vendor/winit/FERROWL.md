# winit 0.30.13, patched for Ferrowl

Unchanged copy of winit 0.30.13 from crates.io plus one fix, marked
"Ferrowl patch" in the source:

- `src/platform_impl/linux/wayland/window/state.rs`: the time a frame
  callback was requested is kept; `frame_callback_overdue()` tells when it is
  more than one second late.
- `src/platform_impl/linux/wayland/event_loop/mod.rs`: a redraw throttled by a
  pending frame callback goes ahead once that callback is overdue.

Why: winit throttles every `RedrawRequested` until the compositor answers the
last frame callback. GNOME (mutter) can drop that answer for a window left in
the background; the window then reads input but never paints again, not even
on resize, until the app is restarted.

Drop this directory and the `[patch.crates-io]` entry in the root Cargo.toml
when iced moves to a winit release that handles a missing frame callback.
