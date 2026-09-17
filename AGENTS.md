# AGENTS.md

This file provides guidance to the agent when working with code in this repository.

## Build, lint, and test

Standard Cargo, but the lint bar and one test workflow are non-standard:

- **Toolchain:** Rust ≥ 1.93 (edition 2024). No `rust-toolchain.toml`; CI uses stable.
- **Format check:** `cargo fmt --all -- --check`
- **Clippy:** `cargo clippy --all-targets -- -D warnings` — warnings are errors in CI. `Cargo.toml` enables `clippy::pedantic` and `clippy::perf` as warn, plus `missing_debug_implementations`, `unsafe_op_in_unsafe_fn`, `unused_lifetimes`. New public types must derive `Debug`; keep clippy clean.
- **Tests:** `cargo test --all-targets`. The suite is headless — no TTY required. View tests build a real `Terminal` over an in-memory backend; protocol tests bind ephemeral loopback ports (control port `0`).
- **Protocol suite is run 10× in CI** (`for i in $(seq 10); do cargo test --test protocol || exit 1; done`) because it is concurrent (thread-per-connection, session reaping on a timer, a self-connect used to wake a blocked `accept`). A single local failure is often a race — rerun before investigating.
- **Golden test regen:** `tests/golden.rs` pins a recorded stream's rendered cell grid against `tests/fixtures/stream.golden`. When the rendered output changes intentionally, regenerate with `PLANK_REGEN_GOLDEN=1 cargo test -p tdk`.

## Architecture

A Turbo Vision TUI that listens on a fixed loopback control port (7878), performs a line handshake, allocates a per-session data port, and renders the streamed bytes in a window. The crate is both a library (`tdk`, `src/lib.rs`) and a binary (`src/main.rs`); the lib exists so integration tests drive the pipeline and protocol without a terminal.

**Data flow** (`src/pipeline.rs` is the spine):

```
bytes → trace_stream::StreamRenderer → TokenRenderer<Vec<u8>> → ANSI
      → AnsiLineAssembler → Vec<Cell> → StreamView
```

The renderer is the *same* `trace_stream` code the `plank` agent uses for its terminal output, given a `Vec<u8>` instead of a terminal. **Do not reimplement rendering here** — the shared state machine is the point; rendering changes belong upstream in `trace-stream`. `Pipeline` also holds `utf8_carry` because `StreamRenderer::push` takes `&str` and bytes arrive UTF-8-split at arbitrary boundaries; a lossy conversion would corrupt multibyte characters.

**Session lifecycle** (`src/registry.rs`): one control listener thread does the handshake and, per session, spawns a dedicated data listener. A session outlives its data socket — a client that drops can reconnect to the same port and rejoin the same window below a `-- reconnected --` rule. Thread growth is unbounded by design (one per control connection, one per session accept loop, one per data connection) — this is a deliberate scope decision, not a resource pool. `ServerEvent` (`Opened`/`Attached`/`Bytes`/`Disconnected`/`Closed`) is the only channel from server to UI. A `LiveGuard` plus a monotonically-increasing `generation` counter protect against the reconnect race where a stale guard's asynchronously-observed EOF would otherwise clobber a newer attachment — don't simplify the generation check. `Server::reap` drops sessions idle > 30 min; the UI calls it every 60 s.

**Decision/apply seam** (`src/main.rs`): `Console::decide_server_event` turns a `ServerEvent` into a `ConsoleIntent` (`CreateWindow`/`Retitle`/`CloseWindow`) carrying no `Application` reference; `apply_intent` then mutates the desktop. The decision side is the testable seam — `console_decision_tests` exercises it without a TTY. When changing what a server event means for the desktop, edit `decide_server_event` and its tests, and keep `Application` out of it.

**Renderers by kind** (`src/session.rs`): a `tokens` session owns a `Pipeline`; a `trace` session owns a `TraceRenderer` (`src/tracefmt.rs`, one colored line per `tracing` JSON record) and never constructs a `Pipeline`. `SharedStreamView` (`Rc<RefCell<StreamView>>`) lets both the desktop and the event pump address the same view.

**StreamView** (`src/streamview.rs`, the largest file): the scrollback — styled cells, one `Vec<Cell>` per logical line, soft-wrapping to view width, vertical-only scroll, a scrollbar, and stream/block text selection.

## Setup and environment

- **No environment variables or config files.** The control port is hardcoded (`CONTROL_PORT = 7878` in `src/main.rs`). The binary takes no options other than `--version`/`--help`.
- `--version`/`--help` are handled *before* `Application::new` (which puts the terminal in raw mode), because packagers smoke-test the binary with no TTY. Any new CLI flag must stay on that pre-UI path.
- CI matrix is **macOS and Ubuntu only** — no Windows testing.

## Gotchas and workflow quirks

- **The console never learns which tool-call dialect a session speaks.** `HELLO` carries a version, a kind and a name, and nothing else; a raw `nc` stream carries not even that. So every dialect is matched at the *opener*, upstream in `trace-stream`, and the one the opener was spelled in is adopted for that stanza — three of them can follow each other in one window. `DeepSeek` V4 (`<｜DSML｜tool_calls>`) and V4.1 Flash (`<｜DSML｜ calls>`, a leading space and a shorter outer tag) are the two DSML spellings; Qwen3.8 (`<tool_call>` / `<function=…>` / `<parameter=…>`) is not DSML-shaped at all and swaps in its own parser and its own banner scan. Needs `trace-stream` ≥ 0.1.5 (≥ 0.1.4 for V4.1 alone); 0.1.5 is on crates.io. Do not add a dialect flag, a `HELLO` field or a menu toggle here: the stream answers the question by itself. The price of Qwen's opener is that it carries no marker token, so a model writing `<tool_call>` in prose trips the detector — a known, deliberate trade.
- **`tool_names()` in `src/pipeline.rs` is a hardcoded copy of plank's tool dispatch table** so DSML tool-call banners match what plank would render. If plank adds a tool, update this list by hand.
- **App command IDs start at 1000** (`src/cmd.rs`) to clear Turbo Vision's reserved low range. Do **not** reuse the library's `CM_TILE`/`CM_CASCADE`: `Application::idle()` re-enables those every poll timeout, clobbering any "more than one window" rule. Use the app-owned `CM_TILE_WINDOWS`/`CM_CASCADE_WINDOWS`, which nothing else touches.
- **Window > Next is the app-owned `CM_NEXT_WINDOW`, never Turbo Vision's `CM_NEXT`.** The desktop logo (`src/logo.rs`) is a desktop child at z-index 1, right above the built-in background; the library's `CM_NEXT` cycles by sending the top window to index 1, i.e. *under* the logo, which would hide it behind the wallpaper. `Console::next_window` cycles the tracked windows in creation order with `Desktop::bring_to_front`, which only ever moves a window up. Any new command that reorders windows must do the same. Everything else in `Desktop` skips index 0 only, so the logo counts as a child there: it has no options (so `count_tileable_windows` and click-to-raise ignore it), cannot focus, and handles no events.
- **`sync_command_state` must run before `handle_event`**, not only before draw. Menu dropdowns paint inside `handle_event`, and `idle()` (called within `get_event`) clobbers tile/cascade enabled-state on every poll timeout — syncing only before draw shows the clobbered state in the dropdown.
- **`MAX_EVENTS_PER_TICK` (64) is load-bearing.** The server event channel is unbounded; without the per-tick cap, a fast producer (`cat bigfile | nc …`) would spin the inner drain loop forever and starve keystrokes, redraws, and Alt-X. Don't remove or raise it casually.
- **Dropping `Server` does not stop the control listener thread** — there is no shutdown handshake for the control listener itself. Only per-session data listeners are torn down, via `reap`. This is a deliberate narrower scope, not a bug.
- **`format_thinking` and `format_markdown` are always on** (`RENDER_OPTIONS` in `src/main.rs`) — deliberately not user-toggleable. Don't re-add View toggles for them.
- **The main loop is hand-rolled**, not `Application::run`. It must call `app.desktop.handle_moved_windows` and `app.desktop.remove_closed_windows` each iteration (the latter triggers `Console::forget_closed_windows`); omitting either leaks stale move-tracking state or closed-window mappings.
- **Session idle TTL is 30 min**, reaped every 60 s from the main loop. `Server::reap` emits `Closed` events on the same channel as everything else.
