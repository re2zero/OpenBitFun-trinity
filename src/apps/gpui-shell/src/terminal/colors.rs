//! Terminal color resolution: ANSI palette (`vte::ansi::Color`) to gpui
//! colors. The 16-color palette, foreground/background, and cursor come from
//! the shell theme side; indexed (256) and direct (24-bit) colors follow the
//! xterm formulas.

use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui_kit::Hsla;

/// Classic xterm base-16 palette (xterm-256color defaults), ordered
/// black..white then bright black..bright white.
pub const XTERM_BASE16: [u32; 16] = [
    0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
    0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
];

/// The six intensity levels of the xterm 6x6x6 color cube.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Theme-side palette: shell foreground/background/cursor plus the 16 ANSI
/// slots. Terminal apps stay readable against the active shell theme while
/// full-fidelity indexed/direct colors bypass the palette.
#[derive(Clone, Copy)]
pub struct TerminalPalette {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub ansi16: [Hsla; 16],
}

impl TerminalPalette {
    /// Palette from shell theme colors with the standard xterm base 16.
    pub fn from_theme(foreground: Hsla, background: Hsla, cursor: Hsla) -> Self {
        Self {
            foreground,
            background,
            cursor,
            ansi16: XTERM_BASE16.map(hsla_from_rgb24),
        }
    }

    /// Resolves one cell color. `dim` applies the SGR 2 attribute (the cell
    /// carries the base color; dimming is a rendering decision).
    pub fn resolve(&self, color: Color, dim: bool) -> Hsla {
        let resolved = match color {
            Color::Named(NamedColor::Foreground | NamedColor::BrightForeground) => self.foreground,
            Color::Named(NamedColor::Background) => self.background,
            Color::Named(NamedColor::Cursor) => self.cursor,
            Color::Named(NamedColor::DimForeground) => dim_color(self.foreground),
            Color::Named(named) => match named_index(named) {
                Some(index) => {
                    let base = self.ansi16[index];
                    if named_is_dim(named) {
                        dim_color(base)
                    } else {
                        base
                    }
                }
                None => self.foreground,
            },
            Color::Indexed(index) => indexed_hsla(index),
            Color::Spec(Rgb { r, g, b }) => {
                hsla_from_rgb24((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
            }
        };
        if dim {
            dim_color(resolved)
        } else {
            resolved
        }
    }
}

/// xterm indexed color (0-255): base 16, 6x6x6 cube, then grayscale ramp.
pub fn indexed_rgb24(index: u8) -> u32 {
    match index {
        0..=15 => XTERM_BASE16[usize::from(index)],
        16..=231 => {
            let n = usize::from(index) - 16;
            let r = CUBE_LEVELS[n / 36];
            let g = CUBE_LEVELS[(n / 6) % 6];
            let b = CUBE_LEVELS[n % 6];
            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
        }
        232..=255 => {
            let gray = (8 + 10 * (usize::from(index) - 232)) as u32;
            (gray << 16) | (gray << 8) | gray
        }
    }
}

fn indexed_hsla(index: u8) -> Hsla {
    hsla_from_rgb24(indexed_rgb24(index))
}

/// Multiplies the RGB channels toward black, approximating alacritty's dim
/// derivation without adding a second palette.
fn dim_color(color: Hsla) -> Hsla {
    let mut color = color;
    color.l *= 0.66;
    color
}

fn hsla_from_rgb24(rgb: u32) -> Hsla {
    gpui_kit::rgb(rgb).into()
}

/// `NamedColor` slot in the base-16 palette, `None` for the non-color
/// specials (foreground/background/cursor and their dim variants).
fn named_index(named: NamedColor) -> Option<usize> {
    match named {
        NamedColor::Black => Some(0),
        NamedColor::Red => Some(1),
        NamedColor::Green => Some(2),
        NamedColor::Yellow => Some(3),
        NamedColor::Blue => Some(4),
        NamedColor::Magenta => Some(5),
        NamedColor::Cyan => Some(6),
        NamedColor::White => Some(7),
        NamedColor::BrightBlack => Some(8),
        NamedColor::BrightRed => Some(9),
        NamedColor::BrightGreen => Some(10),
        NamedColor::BrightYellow => Some(11),
        NamedColor::BrightBlue => Some(12),
        NamedColor::BrightMagenta => Some(13),
        NamedColor::BrightCyan => Some(14),
        NamedColor::BrightWhite => Some(15),
        _ => None,
    }
}

fn named_is_dim(named: NamedColor) -> bool {
    matches!(
        named,
        NamedColor::DimBlack
            | NamedColor::DimRed
            | NamedColor::DimGreen
            | NamedColor::DimYellow
            | NamedColor::DimBlue
            | NamedColor::DimMagenta
            | NamedColor::DimCyan
            | NamedColor::DimWhite
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_palette_matches_xterm_formulas() {
        assert_eq!(indexed_rgb24(0), 0x000000);
        assert_eq!(indexed_rgb24(1), 0xcd0000);
        assert_eq!(indexed_rgb24(15), 0xffffff);
        // Cube corner 196 = 16 + 180 = r:5,g:0,b:0 -> full red.
        assert_eq!(indexed_rgb24(196), 0xff0000);
        // Cube 46 = 16 + 30 = r:0,g:5,b:0 -> full green.
        assert_eq!(indexed_rgb24(46), 0x00ff00);
        // Grayscale ramp starts at 8 and steps by 10.
        assert_eq!(indexed_rgb24(232), 0x080808);
        assert_eq!(indexed_rgb24(255), 0xeeeeee);
    }

    #[test]
    fn resolve_maps_named_and_dim() {
        let palette = TerminalPalette::from_theme(
            gpui_kit::rgb(0xffffff).into(),
            gpui_kit::rgb(0x101014).into(),
            gpui_kit::rgb(0x60a5fa).into(),
        );
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Red), false),
            palette.ansi16[1]
        );
        let dim_red = palette.resolve(Color::Named(NamedColor::Red), true);
        assert!(dim_red.l < palette.ansi16[1].l, "dim must darken");
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Background), false),
            palette.background
        );
        // Indexed 196 is full red in the cube.
        assert_eq!(palette.resolve(Color::Indexed(196), false).l, 0.5);
    }
}
