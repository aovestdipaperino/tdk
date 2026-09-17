// Copyright (c) 2026 Enzo Lombardi
// SPDX-License-Identifier: MIT

//! Application command ids.
//!
//! Turbo Vision reserves the low range for built-ins (`CM_QUIT`, `CM_CLOSE`,
//! `CM_TILE`, ...). These start at 1000 to stay clear of them.

use turbo_vision::core::command::CommandId;

/// File > Open capture...
pub const CM_OPEN_CAPTURE: CommandId = 1000;
/// File > Save As...
pub const CM_SAVE_AS: CommandId = 1001;
/// Edit > Clear window
pub const CM_CLEAR_WINDOW: CommandId = 1004;
/// Edit > Copy (copies the current selection to the clipboard)
pub const CM_COPY: CommandId = 1002;
/// Edit > Select All (selects the whole scrollback)
pub const CM_SELECT_ALL: CommandId = 1003;
/// Window > Cleanup (closes every disconnected window)
pub const CM_CLEANUP: CommandId = 1005;
/// Window > Tile (an app-owned id — see `CM_CASCADE_WINDOWS`)
pub const CM_TILE_WINDOWS: CommandId = 1006;
/// Window > Cascade.
///
/// Deliberately *not* Turbo Vision's own `CM_TILE` / `CM_CASCADE`:
/// `Application::idle()` re-enables those two ids whenever the desktop holds
/// any tileable window at all, and it runs inside `Application::get_event`
/// on every poll timeout — so an app-level "only with more than one window"
/// rule applied to the library ids is overwritten milliseconds later. These
/// ids are ours alone; nothing else touches their enabled state, and
/// `Console::handle_command` maps them onto `Application::tile` / `cascade`.
pub const CM_CASCADE_WINDOWS: CommandId = 1007;

/// Window > Auto-cleanup (a flag: close each window as its client goes away)
pub const CM_AUTO_CLEANUP: CommandId = 1008;
/// Window > Auto-tile (a flag: re-tile the desktop whenever a window opens
/// or closes, in creation order, left-to-right then top-to-bottom)
pub const CM_AUTO_TILE: CommandId = 1009;

/// Window > Next (F6).
///
/// Not Turbo Vision's `CM_NEXT`: the desktop implements that by sending the
/// top window to z-index 1, which is *under* the [`crate::logo::Logo`] view
/// that sits there, so the window would vanish behind the wallpaper. This id
/// is handled by `Console::handle_command` with `Desktop::bring_to_front`,
/// cycling the tracked windows in creation order.
pub const CM_NEXT_WINDOW: CommandId = 1010;
