//! HLE BIOS boot animation: the GBA-WASM logo zooms in, the wordmark is typed out, and the
//! screen fades to black before the cartridge starts.
//!
//! The animation only programs emulated hardware (palette, VRAM, OAM, and display
//! registers), so the PPU draws it like any game screen: a Mode 4 bitmap holds the background
//! and wordmark, an affine double-size object holds the logo, and the fades use the
//! brightness effect. The CPU does not run during the animation. `GbaCore` then restores the
//! post-BIOS state, so a game starts exactly as it does without the animation.

use crate::boot_logo::{LOGO_PALETTE, LOGO_PIXELS};
use crate::cpu::{affine_matrix, integer_square_root};
use crate::memory::{DISPCNT_ADDR, KEYINPUT_ADDR, MemoryMap};

/// About 1.9 seconds at 59.73 fps.
pub const INTRO_FRAMES: u32 = 116;
const FADE_IN_END: u32 = 16;
const ZOOM_END: u32 = 40;
const TEXT_START: u32 = 40;
const FRAMES_PER_LETTER: u32 = 3;
const FADE_OUT_START: u32 = 100;

const SCREEN_WIDTH: i32 = 240;
const SCREEN_HEIGHT: i32 = 160;
const LOGO_SIZE: i32 = 64;
const LOGO_CENTER: (i32, i32) = (120, 66);

// Mode 4, 1D object mapping, BG2 and objects on.
const DISPLAY_CONTROL: u16 = 0x0004 | 1 << 6 | 1 << 10 | 1 << 12;
const BLDCNT_ADDR: u32 = 0x0400_0050;
const BLDY_ADDR: u32 = 0x0400_0054;
// Brightness decrease on BG2, objects, and the backdrop.
const BLEND_CONTROL: u16 = 1 << 2 | 1 << 4 | 1 << 5 | 3 << 6;
/// Object tiles in bitmap modes start at tile 512 (VRAM `0x14000`).
const LOGO_TILE: u16 = 512;
const OBJ_PALETTE_OFFSET: usize = 0x200;

// Mode 4 background palette: a radial gradient, then the wordmark colors.
const GRADIENT_LEVELS: u32 = 8;
const GRADIENT_CENTER: [u32; 3] = [0x49, 0x37, 0x5D];
const GRADIENT_EDGE: [u32; 3] = [0x21, 0x1D, 0x29];
const TEXT_COLOR: u8 = GRADIENT_LEVELS as u8 + 1;
const ACCENT_COLOR: u8 = TEXT_COLOR + 1;
const SHADOW_COLOR: u8 = TEXT_COLOR + 2;
const TEXT_PALETTE: [[u32; 3]; 3] = [[0xE8, 0xD7, 0xFA], [0xBB, 0x8D, 0xF2], [0x17, 0x11, 0x1F]];
const BAYER_4X4: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

const WORDMARK: &[u8] = b"GBA-WASM";
const GLYPH_SCALE: i32 = 2;
const GLYPH_ADVANCE: i32 = 6 * GLYPH_SCALE;
const WORDMARK_TOP: i32 = 112;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootIntro {
    frame: u32,
    /// Keys held on the previous frame, so a skip needs a fresh press.
    held_keys: u16,
}

impl BootIntro {
    pub fn new() -> Self {
        Self {
            frame: 0,
            held_keys: 0,
        }
    }

    /// Programs the hardware for the next frame. Returns false, without drawing, once the
    /// animation has ended or a newly pressed key skips it.
    pub fn draw_next_frame(&mut self, memory: &mut MemoryMap) -> bool {
        let held = !memory.read_u16(KEYINPUT_ADDR) & 0x03FF;
        // Keys already held when the animation starts (such as the one that launched the
        // game) do not skip it.
        let newly_pressed = if self.frame == 0 {
            0
        } else {
            held & !self.held_keys
        };
        self.held_keys = held;
        if self.frame >= INTRO_FRAMES || newly_pressed != 0 {
            return false;
        }

        if self.frame == 0 {
            set_up_screen(memory);
        }
        set_logo_transform(memory, self.frame);
        draw_wordmark(memory, self.frame);
        memory.write_u16(BLDY_ADDR, brightness(self.frame));
        self.frame += 1;
        true
    }
}

fn rgb555([red, green, blue]: [u32; 3]) -> u16 {
    (red >> 3 | (green >> 3) << 5 | (blue >> 3) << 10) as u16
}

fn write_palette(palette: &mut [u8], index: usize, color: u16) {
    palette[index * 2..index * 2 + 2].copy_from_slice(&color.to_le_bytes());
}

fn set_up_screen(memory: &mut MemoryMap) {
    for level in 0..GRADIENT_LEVELS {
        let mix = |channel: usize| {
            let (center, edge) = (GRADIENT_CENTER[channel], GRADIENT_EDGE[channel]);
            center - (center - edge) * level / (GRADIENT_LEVELS - 1)
        };
        write_palette(
            &mut memory.palette,
            level as usize + 1,
            rgb555([mix(0), mix(1), mix(2)]),
        );
    }
    // The backdrop matches the outermost gradient level.
    write_palette(&mut memory.palette, 0, rgb555(GRADIENT_EDGE));
    for (offset, color) in TEXT_PALETTE.into_iter().enumerate() {
        write_palette(
            &mut memory.palette,
            usize::from(TEXT_COLOR) + offset,
            rgb555(color),
        );
    }
    for (index, color) in LOGO_PALETTE.into_iter().enumerate() {
        write_palette(&mut memory.palette[OBJ_PALETTE_OFFSET..], index, color);
    }

    // Elliptical gradient around the screen center, ordered-dithered between levels.
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let (dx, dy) = (x - SCREEN_WIDTH / 2, y - SCREEN_HEIGHT / 2);
            // Squared distance in 1/65536 units of the ellipse radii (124 x 83 pixels).
            let squared = (dx * dx * 65536 / (124 * 124) + dy * dy * 65536 / (83 * 83)) as u32;
            let distance = integer_square_root(squared.min(65536)); // 0..=256
            let threshold = BAYER_4X4[y as usize & 3][x as usize & 3] * 16 + 8;
            let level =
                ((distance * (GRADIENT_LEVELS - 1) + threshold) / 256).min(GRADIENT_LEVELS - 1);
            memory.vram[(y * SCREEN_WIDTH + x) as usize] = level as u8 + 1;
        }
    }

    // The logo as 4bpp tiles in 1D order: 8x8 tiles, each 4 bytes per row.
    let tiles = LOGO_SIZE as usize / 8;
    let tile_base = 0x10000 + usize::from(LOGO_TILE) * 32;
    for (y, row) in LOGO_PIXELS.iter().enumerate() {
        for (x, digit) in row.bytes().enumerate() {
            let index = (digit as char).to_digit(16).unwrap() as u8;
            let tile = (y / 8) * tiles + x / 8;
            let offset = tile_base + tile * 32 + (y % 8) * 4 + (x % 8) / 2;
            memory.vram[offset] |= index << ((x % 2) * 4);
        }
    }

    // Object 0 is the logo; every other object is disabled.
    for object in 1..128 {
        memory.oam[object * 8..object * 8 + 2].copy_from_slice(&(1u16 << 9).to_le_bytes());
    }
    let half_box = LOGO_SIZE; // a double-size box is twice the sprite size
    let attribute0 = (LOGO_CENTER.1 - half_box) as u16 & 0xFF | 1 << 8 | 1 << 9;
    let attribute1 = (LOGO_CENTER.0 - half_box) as u16 & 0x1FF | 3 << 14;
    for (index, attribute) in [attribute0, attribute1, LOGO_TILE].into_iter().enumerate() {
        memory.oam[index * 2..index * 2 + 2].copy_from_slice(&attribute.to_le_bytes());
    }

    memory.write_u16(DISPCNT_ADDR, DISPLAY_CONTROL);
    memory.write_u16(BLDCNT_ADDR, BLEND_CONTROL);
}

/// Spins and zooms the logo from a point to full size with an ease-out curve.
fn set_logo_transform(memory: &mut MemoryMap, frame: u32) {
    let progress = frame.min(ZOOM_END) * 256 / ZOOM_END;
    let remaining = 256 - progress;
    let scale = 256 - remaining * remaining * remaining / 65536; // 8.8 fixed point
    let texture_step = (65536 / scale.max(8)) as i32;
    let angle = ((256 - scale) * 0x80) as u16; // half a turn at the start
    for (index, parameter) in affine_matrix(texture_step, texture_step, angle)
        .into_iter()
        .enumerate()
    {
        let offset = index * 8 + 6;
        memory.oam[offset..offset + 2].copy_from_slice(&(parameter as u16).to_le_bytes());
    }
}

/// Reveals one letter every few frames, drawn at twice the font size with a drop shadow.
fn draw_wordmark(memory: &mut MemoryMap, frame: u32) {
    if frame < TEXT_START {
        return;
    }
    let letters = (((frame - TEXT_START) / FRAMES_PER_LETTER + 1) as usize).min(WORDMARK.len());
    let width = WORDMARK.len() as i32 * GLYPH_ADVANCE - GLYPH_SCALE;
    let left = (SCREEN_WIDTH - width) / 2;
    for (position, &letter) in WORDMARK[..letters].iter().enumerate() {
        let color = if letter == b'-' {
            ACCENT_COLOR
        } else {
            TEXT_COLOR
        };
        let x = left + position as i32 * GLYPH_ADVANCE;
        for (dx, dy, color) in [(1, 2, SHADOW_COLOR), (0, 0, color)] {
            draw_glyph(memory, letter, x + dx, WORDMARK_TOP + dy, color);
        }
    }
}

fn draw_glyph(memory: &mut MemoryMap, letter: u8, left: i32, top: i32, color: u8) {
    for (row, bits) in glyph(letter).iter().enumerate() {
        for column in 0..5 {
            if bits & (0x10 >> column) == 0 {
                continue;
            }
            for dy in 0..GLYPH_SCALE {
                for dx in 0..GLYPH_SCALE {
                    let x = left + column * GLYPH_SCALE + dx;
                    let y = top + row as i32 * GLYPH_SCALE + dy;
                    memory.vram[(y * SCREEN_WIDTH + x) as usize] = color;
                }
            }
        }
    }
}

/// 5x7 glyphs, one row per byte with the leftmost pixel in bit 4.
fn glyph(letter: u8) -> [u8; 7] {
    match letter {
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'-' => [0x00, 0x00, 0x00, 0x0E, 0x00, 0x00, 0x00],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        _ => [0; 7],
    }
}

/// BLDY: fade in from black, hold, then fade out to black.
fn brightness(frame: u32) -> u16 {
    let level = if frame < FADE_IN_END {
        16 - frame * 16 / FADE_IN_END
    } else if frame >= FADE_OUT_START {
        ((frame - FADE_OUT_START + 1) * 16 / (INTRO_FRAMES - FADE_OUT_START)).min(16)
    } else {
        0
    };
    level as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_fades_in_holds_and_ends_black() {
        assert_eq!(brightness(0), 16);
        assert_eq!(brightness(FADE_IN_END), 0);
        assert_eq!(brightness(FADE_OUT_START - 1), 0);
        assert_eq!(brightness(INTRO_FRAMES - 1), 16);
    }

    #[test]
    fn logo_reaches_identity_scale_without_rotation() {
        let mut memory = MemoryMap::new();
        set_logo_transform(&mut memory, ZOOM_END);
        let parameter = |index: usize| {
            u16::from_le_bytes([memory.oam[index * 8 + 6], memory.oam[index * 8 + 7]])
        };
        assert_eq!(
            [parameter(0), parameter(1), parameter(2), parameter(3)],
            [0x100, 0, 0, 0x100]
        );
    }

    #[test]
    fn logo_pixels_are_square_hex_rows() {
        assert!(LOGO_PIXELS.iter().all(|row| row.len() == LOGO_SIZE as usize
            && row.bytes().all(|digit| digit.is_ascii_hexdigit())));
    }

    #[test]
    fn a_new_key_press_skips_but_a_held_key_does_not() {
        let mut memory = MemoryMap::new();
        memory.set_pressed_keys(1 << 3); // Start held from before the intro
        let mut intro = BootIntro::new();
        assert!(intro.draw_next_frame(&mut memory));
        assert!(intro.draw_next_frame(&mut memory));
        memory.set_pressed_keys(1 << 3 | 1);
        assert!(!intro.draw_next_frame(&mut memory));
    }
}
