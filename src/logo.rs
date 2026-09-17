// Copyright (c) 2026 Enzo Lombardi
// SPDX-License-Identifier: MIT

//! The desktop wallpaper: Turbo Vision's `░` pattern with a big navy "TDK"
//! in the middle, drawn with quadrant blocks at twice the cell resolution. Modelled on the library's `desktop_logo` example, which adds a
//! logo view as the first desktop child on top of the stock background.

use turbo_vision::core::draw::DrawBuffer;
use turbo_vision::core::event::Event;
use turbo_vision::core::geometry::Rect;
use turbo_vision::core::palette::{Attr, Palette, TvColor, colors};
use turbo_vision::core::state::{GF_GROW_HI_X, GF_GROW_HI_Y, GrowFlags, StateFlags};
use turbo_vision::terminal::Terminal;
use turbo_vision::views::view::{View, write_line_to_terminal};

/// "TDK" as a 1-bit bitmap, `#` = ink: Arial Black rasterised to 116 x 36
/// pixels. Two bitmap pixels per cell in each direction: the renderer packs a
/// 2x2 block into one quadrant character, so this draws at twice the
/// terminal's cell resolution. Every row is the same width and the row count
/// is even.
const BITMAP: [&str; 36] = [
    "##################################.....######################.................###########...........###############.",
    "##################################.....#########################..............###########..........###############..",
    "##################################.....##########################.............###########.........###############...",
    "##################################.....############################...........###########.........##############....",
    "##################################.....#############################..........###########........##############.....",
    "##################################.....#############################..........###########.......##############......",
    "##################################.....##############################.........###########......##############.......",
    "##################################.....###############################........###########.....##############........",
    "............###########................###########....################........###########....##############.........",
    "............###########................###########........#############.......###########....#############..........",
    "............###########................###########.........############.......###########...##############..........",
    "............###########................###########.........############.......###########..##############...........",
    "............###########................###########..........###########.......###########.##############............",
    "............###########................###########..........############......#########################.............",
    "............###########................###########..........############......#########################.............",
    "............###########................###########...........###########......##########################............",
    "............###########................###########...........###########......##########################............",
    "............###########................###########...........###########......###########################...........",
    "............###########................###########...........###########......###########################...........",
    "............###########................###########...........###########......############################..........",
    "............###########................###########...........###########......#############################.........",
    "............###########................###########..........############......#############################.........",
    "............###########................###########..........############......################.#############........",
    "............###########................###########..........############......###############...############........",
    "............###########................###########..........###########.......##############....#############.......",
    "............###########................###########.........############.......##############.....#############......",
    "............###########................###########........#############.......#############......#############......",
    "............###########................###########.....###############........############........#############.....",
    "............###########................###############################........###########.........#############.....",
    "............###########................##############################.........###########..........#############....",
    "............###########................#############################..........###########..........##############...",
    "............###########................#############################..........###########...........#############...",
    "............###########................############################...........###########...........##############..",
    "............###########................##########################.............###########............##############.",
    "............###########................########################...............###########.............#############.",
    "............###########................#####################..................###########.............##############",
];

/// Quadrant block for a 2x2 pixel group; bit 1 = top-left, 2 = top-right,
/// 4 = bottom-left, 8 = bottom-right. Index 0 is unused (wallpaper shows).
const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// Colour of the letters over the desktop's dark grey.
const LOGO_ATTR: Attr = Attr::new(TvColor::Rgb { r: 0, g: 0, b: 96 }, TvColor::DarkGray);

/// A full-desktop view: the stock `░` wallpaper with the logo centred on it.
///
/// It sits at desktop index 1, right above the built-in background, and is
/// deliberately inert: no options (so a click on empty desktop does not raise
/// it over a window), no focus, no event handling. It grows with the desktop
/// on a terminal resize, like the built-in background.
///
/// One thing must never happen: Turbo Vision's own `CM_NEXT` cycles windows
/// by sending the top one *to index 1*, i.e. underneath this view, which
/// would hide it behind the wallpaper. That is why the app's Window > Next is
/// `cmd::CM_NEXT_WINDOW`, an id the desktop does not know, handled with
/// `Desktop::bring_to_front`. That call only ever moves windows *up*.
pub struct Logo {
    bounds: Rect,
    state: StateFlags,
    palette_chain: Option<turbo_vision::core::palette_chain::PaletteChainNode>,
}

impl std::fmt::Debug for Logo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Logo")
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

impl Logo {
    /// A logo view covering `bounds` (desktop-relative: pass `0,0,w,h`).
    #[must_use]
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            state: 0,
            palette_chain: None,
        }
    }

    /// Size of the letter block, in cells: `(width, height)`.
    #[must_use]
    pub fn letters_size() -> (usize, usize) {
        (
            BITMAP
                .iter()
                .map(|l| l.len())
                .max()
                .unwrap_or(0)
                .div_ceil(2),
            BITMAP.len().div_ceil(2),
        )
    }

    /// Is bitmap pixel `(x, y)` ink? Out of range counts as blank.
    fn ink(x: usize, y: usize) -> bool {
        BITMAP
            .get(y)
            .is_some_and(|row| row.as_bytes().get(x) == Some(&b'#'))
    }

    /// Render one desktop row into `buf`: wallpaper everywhere, and the
    /// matching slice of the letters if `row` falls inside the centred block.
    /// Pure, so a test can check the placement without a terminal.
    pub fn render_row(width: usize, height: usize, row: usize, buf: &mut DrawBuffer) {
        buf.move_char(0, '░', colors::DESKTOP, width);
        let (lw, lh) = Self::letters_size();
        if lw > width || lh > height {
            return;
        }
        let x0 = (width - lw) / 2;
        let y0 = (height - lh) / 2;
        if row < y0 || row >= y0 + lh {
            return;
        }
        let py = (row - y0) * 2;
        for i in 0..lw {
            let px = i * 2;
            let bits = usize::from(Self::ink(px, py))
                | usize::from(Self::ink(px + 1, py)) << 1
                | usize::from(Self::ink(px, py + 1)) << 2
                | usize::from(Self::ink(px + 1, py + 1)) << 3;
            if bits != 0 {
                buf.put_char(x0 + i, QUADRANTS[bits], LOGO_ATTR);
            }
        }
    }
}

impl View for Logo {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn state(&self) -> StateFlags {
        self.state
    }

    fn set_state(&mut self, state: StateFlags) {
        self.state = state;
    }

    fn grow_mode(&self) -> GrowFlags {
        GF_GROW_HI_X | GF_GROW_HI_Y
    }

    fn draw(&mut self, terminal: &mut Terminal) {
        let width = usize::try_from(self.bounds.width_clamped()).unwrap_or(0);
        let height = usize::try_from(self.bounds.height()).unwrap_or(0);
        for row in 0..height {
            let mut buf = DrawBuffer::new(width);
            Self::render_row(width, height, row, &mut buf);
            write_line_to_terminal(
                terminal,
                self.bounds.a.x,
                self.bounds.a.y + i16::try_from(row).unwrap_or(i16::MAX),
                &buf,
            );
        }
    }

    fn handle_event(&mut self, _event: &mut Event) {}

    fn set_palette_chain(
        &mut self,
        node: Option<turbo_vision::core::palette_chain::PaletteChainNode>,
    ) {
        self.palette_chain = node;
    }

    fn get_palette_chain(&self) -> Option<&turbo_vision::core::palette_chain::PaletteChainNode> {
        self.palette_chain.as_ref()
    }

    fn get_palette(&self) -> Option<Palette> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_centred_and_wallpaper_fills_the_rest() {
        let (lw, lh) = Logo::letters_size();
        let (w, h) = (80, 23);
        let y0 = (h - lh) / 2;
        let x0 = (w - lw) / 2;
        // A row above the block is plain wallpaper.
        let mut buf = DrawBuffer::new(w);
        Logo::render_row(w, h, y0 - 1, &mut buf);
        assert!((0..w).all(|x| buf.data[x].ch == '░'));
        // The first letter row starts with the top bar of the T.
        let mut buf = DrawBuffer::new(w);
        Logo::render_row(w, h, y0, &mut buf);
        assert_eq!(buf.data[x0].ch, '█');
        assert_eq!(buf.data[x0 - 1].ch, '░');
    }

    #[test]
    fn a_desktop_too_small_for_the_letters_shows_only_wallpaper() {
        let mut buf = DrawBuffer::new(20);
        Logo::render_row(20, 5, 2, &mut buf);
        assert!((0..20).all(|x| buf.data[x].ch == '░'));
    }
}
