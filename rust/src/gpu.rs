//! Scanline PPU: renders each visible line from emulated I/O, VRAM, palette RAM, and OAM.

use crate::memory::MemoryMap;
use crate::state::impl_state;

pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
const DISPLAY_MODE_MASK: u16 = 0x0007;
const DISPLAY_FRAME_SELECT: u16 = 1 << 4;
const DISPLAY_OBJ_1D_MAPPING: u16 = 1 << 6;
const DISPLAY_FORCED_BLANK: u16 = 1 << 7;
const DISPLAY_OBJ_ENABLE: u16 = 1 << 12;
const DISPLAY_WIN0_ENABLE: u16 = 1 << 13;
const DISPLAY_WIN1_ENABLE: u16 = 1 << 14;
const DISPLAY_OBJ_WINDOW_ENABLE: u16 = 1 << 15;
const BITMAP_BACK_FRAME_OFFSET: usize = 0xA000;
const MODE5_WIDTH: usize = 160;
const MODE5_HEIGHT: usize = 128;
const BG_VRAM_END: usize = 0x1_0000;
const OBJ_VRAM_BASE: usize = 0x1_0000;
const OBJ_PALETTE_BASE: usize = 0x200;
/// Colors are 15-bit; bit 15 marks a transparent layer pixel.
const TRANSPARENT: u16 = 0x8000;
const LAYER_OBJ: usize = 4;
const LAYER_BACKDROP: usize = 5;
const WINDOW_EFFECTS: u8 = 1 << 5;
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// OBJ width/height in pixels by shape (square, horizontal, vertical) and size.
const OBJ_DIMENSIONS: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

fn rgb555_to_rgba(color: u16) -> [u8; 4] {
    let expand = |channel: u16| {
        let channel = (channel & 0x1F) as u8;
        (channel << 3) | (channel >> 2)
    };
    [expand(color), expand(color >> 5), expand(color >> 10), 255]
}

fn read_halfword(bytes: &[u8], offset: usize) -> u16 {
    match bytes.get(offset..offset + 2) {
        Some(pair) => u16::from_le_bytes([pair[0], pair[1]]),
        None => 0,
    }
}

fn io_halfword(memory: &MemoryMap, offset: usize) -> u16 {
    read_halfword(&memory.io, offset)
}

fn io_word(memory: &MemoryMap, offset: usize) -> u32 {
    u32::from(io_halfword(memory, offset)) | (u32::from(io_halfword(memory, offset + 2)) << 16)
}

fn palette_color(memory: &MemoryMap, index: usize) -> u16 {
    read_halfword(&memory.palette, index * 2) & 0x7FFF
}

/// Reads a 4bpp or 8bpp palette index from tile data; `None` when at or past `limit`.
fn tile_pixel(
    vram: &[u8],
    tile_address: usize,
    x: usize,
    y: usize,
    color_256: bool,
    limit: usize,
) -> Option<u8> {
    let address = if color_256 {
        tile_address + y * 8 + x
    } else {
        tile_address + y * 4 + x / 2
    };
    if address >= limit {
        return None;
    }
    let byte = vram[address];
    Some(if color_256 {
        byte
    } else if x & 1 == 0 {
        byte & 0x0F
    } else {
        byte >> 4
    })
}

/// Horizontal and vertical mosaic block sizes for backgrounds or objects.
fn mosaic_size(memory: &MemoryMap, objects: bool) -> (usize, usize) {
    let register = io_halfword(memory, 0x04C) >> if objects { 8 } else { 0 };
    (
        usize::from(register & 0xF) + 1,
        usize::from((register >> 4) & 0xF) + 1,
    )
}

#[derive(Debug, Clone, Copy)]
struct ObjPixel {
    color: u16,
    priority: u16,
    semi_transparent: bool,
}

const EMPTY_OBJ_PIXEL: ObjPixel = ObjPixel {
    color: TRANSPARENT,
    priority: 4,
    semi_transparent: false,
};

#[derive(Debug, Clone, Copy)]
struct LayerPixel {
    color: u16,
    layer: usize,
    key: u16,
}

#[derive(Debug, Clone)]
pub struct GpuCore {
    pub framebuffer: Vec<u8>,
    /// Internal BG2/BG3 affine reference point (x, y) in 20.8 fixed point.
    affine_reference: [[i32; 2]; 2],
}

impl_state!(GpuCore {
    framebuffer,
    affine_reference
});

impl GpuCore {
    pub fn new() -> Self {
        Self {
            framebuffer: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            affine_reference: [[0; 2]; 2],
        }
    }

    pub fn clear(&mut self) {
        self.framebuffer.fill(0);
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, rgba: [u8; 4]) {
        if x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT {
            return;
        }

        let index = (y * SCREEN_WIDTH + x) * 4;
        self.framebuffer[index..index + 4].copy_from_slice(&rgba);
    }

    /// Renders all visible lines from the current state, as if nothing changed mid-frame.
    pub fn render(&mut self, memory: &MemoryMap) {
        for line in 0..SCREEN_HEIGHT {
            self.render_scanline(memory, line, [false; 2]);
        }
    }

    /// Renders one visible line. The affine reference points reload from their registers
    /// at line 0 and when `reference_writes` reports a write to BG2X/Y or BG3X/Y.
    pub fn render_scanline(
        &mut self,
        memory: &MemoryMap,
        line: usize,
        reference_writes: [bool; 2],
    ) {
        for (index, written) in reference_writes.iter().enumerate() {
            if line == 0 || *written {
                let base = 0x028 + index * 0x10;
                self.affine_reference[index] = [
                    ((io_word(memory, base) << 4) as i32) >> 4,
                    ((io_word(memory, base + 4) << 4) as i32) >> 4,
                ];
            }
        }

        let display_control = io_halfword(memory, 0x000);
        if display_control & DISPLAY_FORCED_BLANK != 0 {
            let row_start = line * SCREEN_WIDTH * 4;
            for pixel in
                self.framebuffer[row_start..row_start + SCREEN_WIDTH * 4].chunks_exact_mut(4)
            {
                pixel.copy_from_slice(&WHITE);
            }
        } else {
            self.compose_scanline(memory, display_control, line);
        }

        for index in 0..2 {
            let base = 0x020 + index * 0x10;
            self.affine_reference[index][0] += i32::from(io_halfword(memory, base + 2) as i16);
            self.affine_reference[index][1] += i32::from(io_halfword(memory, base + 6) as i16);
        }
    }

    fn compose_scanline(&mut self, memory: &MemoryMap, display_control: u16, line: usize) {
        let mode = display_control & DISPLAY_MODE_MASK;
        let mut backgrounds = [[TRANSPARENT; SCREEN_WIDTH]; 4];
        let mut enabled = [false; 4];
        for (background, pixels) in backgrounds.iter_mut().enumerate() {
            if display_control & (0x100 << background) == 0 {
                continue;
            }
            enabled[background] = match (mode, background) {
                (0, _) | (1, 0 | 1) => {
                    render_text_background(memory, background, line, pixels);
                    true
                }
                (1, 2) | (2, 2 | 3) => {
                    self.render_affine_background(memory, background, pixels);
                    true
                }
                (3..=5, 2) => {
                    self.render_bitmap_background(memory, mode, display_control, pixels);
                    true
                }
                _ => false,
            };
        }

        let mut objects = [EMPTY_OBJ_PIXEL; SCREEN_WIDTH];
        let mut object_window = [false; SCREEN_WIDTH];
        if display_control & DISPLAY_OBJ_ENABLE != 0 {
            render_objects(
                memory,
                display_control,
                line,
                &mut objects,
                &mut object_window,
            );
        }

        let backdrop = palette_color(memory, 0);
        let blend_control = io_halfword(memory, 0x050);
        let blend_mode = (blend_control >> 6) & 3;
        let coefficient = |value: u8| u32::from(value & 0x1F).min(16);
        let eva = coefficient(memory.io[0x052]);
        let evb = coefficient(memory.io[0x053]);
        let evy = coefficient(memory.io[0x054]);
        let priorities: [u16; 4] =
            std::array::from_fn(|background| io_halfword(memory, 0x008 + background * 2) & 3);

        for x in 0..SCREEN_WIDTH {
            let window = window_mask(memory, display_control, line, x, object_window[x]);
            let backdrop_pixel = LayerPixel {
                color: backdrop,
                layer: LAYER_BACKDROP,
                key: u16::MAX,
            };
            let (mut top, mut second) = (backdrop_pixel, backdrop_pixel);
            let mut consider = |candidate: LayerPixel| {
                if candidate.key < top.key {
                    second = top;
                    top = candidate;
                } else if candidate.key < second.key {
                    second = candidate;
                }
            };

            // Objects draw above backgrounds of the same priority; lower BG numbers win ties.
            let object = objects[x];
            if window & (1 << LAYER_OBJ) != 0 && object.color != TRANSPARENT {
                consider(LayerPixel {
                    color: object.color,
                    layer: LAYER_OBJ,
                    key: object.priority * 8,
                });
            }
            for background in 0..4 {
                let color = backgrounds[background][x];
                if enabled[background] && window & (1 << background) != 0 && color != TRANSPARENT {
                    consider(LayerPixel {
                        color,
                        layer: background,
                        key: priorities[background] * 8 + 1 + background as u16,
                    });
                }
            }

            let effects = window & WINDOW_EFFECTS != 0;
            let first_target = blend_control & (1 << top.layer) != 0;
            let second_target = blend_control & (0x100 << second.layer) != 0;
            let semi_transparent = top.layer == LAYER_OBJ && object.semi_transparent;
            let color = if effects && semi_transparent && second_target {
                alpha_blend(top.color, second.color, eva, evb)
            } else if effects && first_target {
                match blend_mode {
                    1 if second_target => alpha_blend(top.color, second.color, eva, evb),
                    2 => adjust_brightness(top.color, evy, true),
                    3 => adjust_brightness(top.color, evy, false),
                    _ => top.color,
                }
            } else {
                top.color
            };

            let index = (line * SCREEN_WIDTH + x) * 4;
            self.framebuffer[index..index + 4].copy_from_slice(&rgb555_to_rgba(color));
        }
    }

    /// Texture coordinate for screen column `x` of an affine layer, in whole pixels.
    fn affine_coordinates(&self, memory: &MemoryMap, background: usize, x: usize) -> (i32, i32) {
        let index = background - 2;
        let base = 0x020 + index * 0x10;
        let pa = i32::from(io_halfword(memory, base) as i16);
        let pc = i32::from(io_halfword(memory, base + 4) as i16);
        let [reference_x, reference_y] = self.affine_reference[index];
        (
            (reference_x + pa * x as i32) >> 8,
            (reference_y + pc * x as i32) >> 8,
        )
    }

    fn render_affine_background(
        &self,
        memory: &MemoryMap,
        background: usize,
        pixels: &mut [u16; SCREEN_WIDTH],
    ) {
        let control = io_halfword(memory, 0x008 + background * 2);
        let char_base = usize::from((control >> 2) & 3) * 0x4000;
        let screen_base = usize::from((control >> 8) & 0x1F) * 0x800;
        let wraps = control & (1 << 13) != 0;
        let size = 128 << (control >> 14);

        for (x, pixel) in pixels.iter_mut().enumerate() {
            let (mut texture_x, mut texture_y) = self.affine_coordinates(memory, background, x);
            if wraps {
                texture_x = texture_x.rem_euclid(size);
                texture_y = texture_y.rem_euclid(size);
            } else if !(0..size).contains(&texture_x) || !(0..size).contains(&texture_y) {
                continue;
            }
            let (texture_x, texture_y) = (texture_x as usize, texture_y as usize);
            let map_address = screen_base + (texture_y / 8) * (size as usize / 8) + texture_x / 8;
            if map_address >= BG_VRAM_END {
                continue;
            }
            let tile = usize::from(memory.vram[map_address]);
            let tile_address = char_base + tile * 64;
            let index = tile_pixel(
                &memory.vram,
                tile_address,
                texture_x % 8,
                texture_y % 8,
                true,
                BG_VRAM_END,
            );
            if let Some(index @ 1..) = index {
                *pixel = palette_color(memory, usize::from(index));
            }
        }
    }

    /// Bitmap modes display BG2 through its affine transform.
    fn render_bitmap_background(
        &self,
        memory: &MemoryMap,
        mode: u16,
        display_control: u16,
        pixels: &mut [u16; SCREEN_WIDTH],
    ) {
        let frame_offset = if mode != 3 && display_control & DISPLAY_FRAME_SELECT != 0 {
            BITMAP_BACK_FRAME_OFFSET
        } else {
            0
        };
        let (width, height) = if mode == 5 {
            (MODE5_WIDTH, MODE5_HEIGHT)
        } else {
            (SCREEN_WIDTH, SCREEN_HEIGHT)
        };

        for (x, pixel) in pixels.iter_mut().enumerate() {
            let (texture_x, texture_y) = self.affine_coordinates(memory, 2, x);
            if !(0..width as i32).contains(&texture_x) || !(0..height as i32).contains(&texture_y) {
                continue;
            }
            let offset = texture_y as usize * width + texture_x as usize;
            *pixel = match mode {
                4 => match memory.vram[frame_offset + offset] {
                    0 => TRANSPARENT,
                    index => palette_color(memory, usize::from(index)),
                },
                _ => read_halfword(&memory.vram, frame_offset + offset * 2) & 0x7FFF,
            };
        }
    }
}

fn render_text_background(
    memory: &MemoryMap,
    background: usize,
    line: usize,
    pixels: &mut [u16; SCREEN_WIDTH],
) {
    let control = io_halfword(memory, 0x008 + background * 2);
    let horizontal_offset = usize::from(io_halfword(memory, 0x010 + background * 4) & 0x1FF);
    let vertical_offset = usize::from(io_halfword(memory, 0x012 + background * 4) & 0x1FF);
    let char_base = usize::from((control >> 2) & 3) * 0x4000;
    let screen_base = usize::from((control >> 8) & 0x1F) * 0x800;
    let color_256 = control & (1 << 7) != 0;
    let (width, height) = match control >> 14 {
        0 => (256, 256),
        1 => (512, 256),
        2 => (256, 512),
        _ => (512, 512),
    };
    let (mosaic_x, mosaic_y) = if control & (1 << 6) != 0 {
        mosaic_size(memory, false)
    } else {
        (1, 1)
    };

    let y = (line - line % mosaic_y + vertical_offset) % height;
    for (x, pixel) in pixels.iter_mut().enumerate() {
        let map_x = (x - x % mosaic_x + horizontal_offset) % width;
        let screen_block = map_x / 256 + (y / 256) * (width / 256);
        let entry_address =
            screen_base + screen_block * 0x800 + ((y % 256) / 8) * 64 + ((map_x % 256) / 8) * 2;
        let entry = read_halfword(&memory.vram, entry_address);
        let tile = usize::from(entry & 0x3FF);
        let tile_x = if entry & (1 << 10) != 0 {
            7 - map_x % 8
        } else {
            map_x % 8
        };
        let tile_y = if entry & (1 << 11) != 0 {
            7 - y % 8
        } else {
            y % 8
        };
        let tile_address = char_base + tile * if color_256 { 64 } else { 32 };

        let index = tile_pixel(
            &memory.vram,
            tile_address,
            tile_x,
            tile_y,
            color_256,
            BG_VRAM_END,
        );
        let Some(index @ 1..) = index else {
            continue;
        };
        let palette_index = if color_256 {
            usize::from(index)
        } else {
            usize::from(entry >> 12) * 16 + usize::from(index)
        };
        *pixel = palette_color(memory, palette_index);
    }
}

fn render_objects(
    memory: &MemoryMap,
    display_control: u16,
    line: usize,
    pixels: &mut [ObjPixel; SCREEN_WIDTH],
    object_window: &mut [bool; SCREEN_WIDTH],
) {
    let one_dimensional = display_control & DISPLAY_OBJ_1D_MAPPING != 0;
    // In bitmap modes the first half of OBJ VRAM holds the bitmap, so tiles 0-511 are unusable.
    let bitmap_mode = display_control & DISPLAY_MODE_MASK >= 3;
    let (mosaic_x, mosaic_y) = mosaic_size(memory, true);

    for index in 0..128 {
        let attribute = |slot: usize| read_halfword(&memory.oam, index * 8 + slot * 2);
        let (attribute0, attribute1, attribute2) = (attribute(0), attribute(1), attribute(2));
        let affine = attribute0 & (1 << 8) != 0;
        let double_size = attribute0 & (1 << 9) != 0;
        if !affine && double_size {
            continue; // disabled
        }
        let object_mode = (attribute0 >> 10) & 3;
        let shape = usize::from(attribute0 >> 14);
        if object_mode == 3 || shape == 3 {
            continue;
        }
        let (width, height) = OBJ_DIMENSIONS[shape][usize::from(attribute1 >> 14)];
        let (box_width, box_height) = if affine && double_size {
            (width * 2, height * 2)
        } else {
            (width, height)
        };

        let top = i32::from(attribute0 & 0xFF);
        let row = (line as i32 - top).rem_euclid(256);
        if row >= box_height {
            continue;
        }
        let mosaic = attribute0 & (1 << 12) != 0;
        let sample_row = if mosaic {
            (row - (line % mosaic_y) as i32).max(0)
        } else {
            row
        };

        let tile_base = usize::from(attribute2 & 0x3FF);
        if bitmap_mode && tile_base < 512 {
            continue;
        }
        let priority = (attribute2 >> 10) & 3;
        let palette_bank = usize::from(attribute2 >> 12);
        let color_256 = attribute0 & (1 << 13) != 0;
        let tile_step = if color_256 { 2 } else { 1 };
        let row_stride = if one_dimensional {
            (width as usize / 8) * tile_step
        } else {
            32
        };
        let left = (i32::from(attribute1 & 0x1FF) << 23) >> 23;

        let [pa, pb, pc, pd] = if affine {
            let group = usize::from((attribute1 >> 9) & 0x1F) * 32;
            std::array::from_fn(|parameter| {
                i32::from(read_halfword(&memory.oam, group + 6 + parameter * 8) as i16)
            })
        } else {
            [0x100, 0, 0, 0x100]
        };
        let horizontal_flip = !affine && attribute1 & (1 << 12) != 0;
        let vertical_flip = !affine && attribute1 & (1 << 13) != 0;

        for column in 0..box_width {
            let screen_x = left + column;
            if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                continue;
            }
            let sample_column = if mosaic && !affine {
                (column - (screen_x as usize % mosaic_x) as i32).max(0)
            } else {
                column
            };
            let (texture_x, texture_y) = if affine {
                let dx = sample_column - box_width / 2;
                let dy = sample_row - box_height / 2;
                (
                    ((pa * dx + pb * dy) >> 8) + width / 2,
                    ((pc * dx + pd * dy) >> 8) + height / 2,
                )
            } else {
                (
                    if horizontal_flip {
                        width - 1 - sample_column
                    } else {
                        sample_column
                    },
                    if vertical_flip {
                        height - 1 - sample_row
                    } else {
                        sample_row
                    },
                )
            };
            if !(0..width).contains(&texture_x) || !(0..height).contains(&texture_y) {
                continue;
            }

            let (texture_x, texture_y) = (texture_x as usize, texture_y as usize);
            let tile =
                (tile_base + (texture_y / 8) * row_stride + (texture_x / 8) * tile_step) & 0x3FF;
            let color_index = tile_pixel(
                &memory.vram,
                OBJ_VRAM_BASE + tile * 32,
                texture_x % 8,
                texture_y % 8,
                color_256,
                memory.vram.len(),
            );
            let Some(color_index @ 1..) = color_index else {
                continue;
            };

            let screen_x = screen_x as usize;
            if object_mode == 2 {
                object_window[screen_x] = true;
                continue;
            }
            let palette_index = if color_256 {
                usize::from(color_index)
            } else {
                palette_bank * 16 + usize::from(color_index)
            };
            // Lower OAM entries win unless a later object has a strictly higher priority.
            let pixel = &mut pixels[screen_x];
            if pixel.color == TRANSPARENT || priority < pixel.priority {
                *pixel = ObjPixel {
                    color: palette_color(memory, OBJ_PALETTE_BASE / 2 + palette_index),
                    priority,
                    semi_transparent: object_mode == 1,
                };
            }
        }
    }
}

/// Layer enable bits (BG0-3, OBJ, effects) for one pixel: WIN0, then WIN1, then the OBJ
/// window, then the outside region. Everything is enabled when no window is active.
fn window_mask(
    memory: &MemoryMap,
    display_control: u16,
    line: usize,
    x: usize,
    in_object_window: bool,
) -> u8 {
    let any_window = DISPLAY_WIN0_ENABLE | DISPLAY_WIN1_ENABLE | DISPLAY_OBJ_WINDOW_ENABLE;
    if display_control & any_window == 0 {
        return 0x3F;
    }
    // A start past the end wraps around the screen edge.
    let inside = |start: usize, end: usize, position: usize, limit: usize| {
        if start <= end {
            position >= start && position < end.min(limit)
        } else {
            position >= start || position < end
        }
    };
    for window in 0..2 {
        if display_control & (DISPLAY_WIN0_ENABLE << window) == 0 {
            continue;
        }
        let horizontal = io_halfword(memory, 0x040 + window * 2);
        let vertical = io_halfword(memory, 0x044 + window * 2);
        let (left, right) = (usize::from(horizontal >> 8), usize::from(horizontal & 0xFF));
        let (top, bottom) = (usize::from(vertical >> 8), usize::from(vertical & 0xFF));
        if inside(left, right, x, SCREEN_WIDTH) && inside(top, bottom, line, SCREEN_HEIGHT) {
            return memory.io[0x048 + window] & 0x3F;
        }
    }
    if display_control & DISPLAY_OBJ_WINDOW_ENABLE != 0 && in_object_window {
        return memory.io[0x04B] & 0x3F;
    }
    memory.io[0x04A] & 0x3F
}

fn channels(color: u16) -> [u32; 3] {
    [
        u32::from(color & 0x1F),
        u32::from((color >> 5) & 0x1F),
        u32::from((color >> 10) & 0x1F),
    ]
}

fn pack(channels: [u32; 3]) -> u16 {
    (channels[0] | (channels[1] << 5) | (channels[2] << 10)) as u16
}

fn alpha_blend(top: u16, bottom: u16, eva: u32, evb: u32) -> u16 {
    let (top, bottom) = (channels(top), channels(bottom));
    pack(std::array::from_fn(|channel| {
        ((top[channel] * eva + bottom[channel] * evb) >> 4).min(31)
    }))
}

fn adjust_brightness(color: u16, evy: u32, brighten: bool) -> u16 {
    pack(channels(color).map(|channel| {
        if brighten {
            channel + (((31 - channel) * evy) >> 4)
        } else {
            channel - ((channel * evy) >> 4)
        }
    }))
}

impl Default for GpuCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::DISPCNT_ADDR;

    const VRAM_BASE: u32 = 0x0600_0000;
    const PALETTE_BASE: u32 = 0x0500_0000;
    const OAM_BASE: u32 = 0x0700_0000;
    const DISPLAY_BG0_ENABLE: u16 = 1 << 8;
    const DISPLAY_BG2_ENABLE: u16 = 1 << 10;
    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn pixel(gpu: &GpuCore, x: usize, y: usize) -> [u8; 4] {
        gpu.framebuffer[(y * SCREEN_WIDTH + x) * 4..][..4]
            .try_into()
            .unwrap()
    }

    /// Fills 4bpp tile `tile` at VRAM offset `base` with palette index `color`.
    fn solid_4bpp_tile(memory: &mut MemoryMap, base: usize, tile: usize, color: u8) {
        for offset in 0..32 {
            memory.vram[base + tile * 32 + offset] = color * 0x11;
        }
    }

    fn disable_objects_from(memory: &mut MemoryMap, first: u32) {
        for object in first..128 {
            memory.write_u16(OAM_BASE + object * 8, 1 << 9);
        }
    }

    #[test]
    fn mode3_decodes_rgb555_pixels_from_vram() {
        let mut memory = MemoryMap::new();
        memory.write_u16(DISPCNT_ADDR, 3 | DISPLAY_BG2_ENABLE);
        memory.write_u16(VRAM_BASE, RED);
        memory.write_u16(VRAM_BASE + 2, GREEN);
        memory.write_u16(VRAM_BASE + 4, BLUE);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(pixel(&gpu, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&gpu, 1, 0), [0, 255, 0, 255]);
        assert_eq!(pixel(&gpu, 2, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn mode4_reads_palette_indices_from_the_selected_frame() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE + 2, RED);
        memory.write_u16(PALETTE_BASE + 4, BLUE);
        memory.vram[0] = 1;
        memory.vram[BITMAP_BACK_FRAME_OFFSET] = 2;
        let mut gpu = GpuCore::new();

        memory.write_u16(DISPCNT_ADDR, 4 | DISPLAY_BG2_ENABLE);
        gpu.render(&memory);
        assert_eq!(pixel(&gpu, 0, 0), [255, 0, 0, 255]);

        memory.write_u16(DISPCNT_ADDR, 4 | DISPLAY_BG2_ENABLE | DISPLAY_FRAME_SELECT);
        gpu.render(&memory);
        assert_eq!(pixel(&gpu, 0, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn mode5_draws_a_160x128_bitmap_inside_the_backdrop() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE, GREEN);
        memory.write_u16(VRAM_BASE + ((MODE5_WIDTH as u32 - 1) * 2), RED);
        memory.write_u16(DISPCNT_ADDR, 5 | DISPLAY_BG2_ENABLE);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(pixel(&gpu, MODE5_WIDTH - 1, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&gpu, MODE5_WIDTH, 0), [0, 255, 0, 255]);
        assert_eq!(pixel(&gpu, 0, MODE5_HEIGHT), [0, 255, 0, 255]);
    }

    #[test]
    fn forced_blank_displays_white() {
        let mut memory = MemoryMap::new();
        memory.write_u16(DISPCNT_ADDR, 3 | DISPLAY_BG2_ENABLE | DISPLAY_FORCED_BLANK);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert!(gpu.framebuffer.iter().all(|byte| *byte == 255));
    }

    #[test]
    fn disabled_layers_show_the_backdrop() {
        let mut memory = MemoryMap::new();
        memory.write_u16(DISPCNT_ADDR, 3);
        memory.write_u16(VRAM_BASE, RED);
        memory.write_u16(PALETTE_BASE, BLUE);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(pixel(&gpu, 0, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn text_background_scrolls_flips_and_uses_its_palette_bank() {
        let mut memory = MemoryMap::new();
        // BG0: char base 0, screen base block 8 (0x4000), 4bpp, 256x256.
        memory.write_u16(0x0400_0008, 8 << 8);
        memory.write_u16(DISPCNT_ADDR, DISPLAY_BG0_ENABLE);
        memory.write_u16(PALETTE_BASE + (2 * 16 + 1) * 2, RED); // bank 2, color 1
        memory.vram[32] = 0x01; // tile 1: only its top-left pixel uses color 1
        // Map entry at tile (1, 0): tile 1, horizontal flip, palette bank 2.
        memory.write_u16(VRAM_BASE + 0x4000 + 2, 1 | (1 << 10) | (2 << 12));
        let mut gpu = GpuCore::new();

        gpu.render(&memory);
        assert_eq!(
            pixel(&gpu, 15, 0),
            [255, 0, 0, 255],
            "flipped pixel lands at x=15"
        );
        assert_eq!(pixel(&gpu, 8, 0), BLACK);

        memory.write_u16(0x0400_0010, 4); // BG0HOFS
        memory.write_u16(0x0400_0012, 256 - 3); // BG0VOFS: map row 0 appears at line 3
        gpu.render(&memory);
        assert_eq!(pixel(&gpu, 11, 3), [255, 0, 0, 255]);
    }

    #[test]
    fn layers_are_ordered_by_priority_and_objects_win_ties() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE + 2, RED);
        memory.write_u16(PALETTE_BASE + 4, GREEN);
        memory.write_u16(PALETTE_BASE + 0x200 + 2, BLUE);
        solid_4bpp_tile(&mut memory, 0, 1, 1);
        solid_4bpp_tile(&mut memory, 0, 2, 2);
        // BG0 (priority 1) shows tile 1 and BG1 (priority 0) shows tile 2 everywhere.
        memory.write_u16(0x0400_0008, (8 << 8) | 1);
        memory.write_u16(0x0400_000A, 9 << 8);
        for entry in 0..32 * 32 {
            memory.write_u16(VRAM_BASE + 0x4000 + entry * 2, 1);
            memory.write_u16(VRAM_BASE + 0x4800 + entry * 2, 2);
        }
        // OBJ 0: 8x8 at (0, 0), OBJ tile 0, priority 0.
        solid_4bpp_tile(&mut memory, OBJ_VRAM_BASE, 0, 1);
        memory.write_u16(OAM_BASE, 0);
        memory.write_u16(OAM_BASE + 2, 0);
        memory.write_u16(OAM_BASE + 4, 0);
        disable_objects_from(&mut memory, 1);
        memory.write_u16(
            DISPCNT_ADDR,
            (0b11 << 8) | DISPLAY_OBJ_ENABLE | DISPLAY_OBJ_1D_MAPPING,
        );
        let mut gpu = GpuCore::new();

        gpu.render(&memory);
        assert_eq!(pixel(&gpu, 0, 0), [0, 0, 255, 255], "OBJ ties BG1 and wins");
        assert_eq!(pixel(&gpu, 8, 0), [0, 255, 0, 255], "BG1 has priority 0");

        memory.write_u16(0x0400_000A, (9 << 8) | 2);
        gpu.render(&memory);
        assert_eq!(
            pixel(&gpu, 8, 0),
            [255, 0, 0, 255],
            "BG0 now has the higher priority"
        );
    }

    #[test]
    fn objects_respect_size_flip_and_disable_flags() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE + 0x200 + 2, RED);
        // 16x8 horizontal OBJ (shape 1, size 0) at x = 100, y = 50, using tiles 0 and 1.
        memory.vram[OBJ_VRAM_BASE] = 0x01; // tile 0, top-left pixel
        memory.write_u16(OAM_BASE, 50 | (1 << 14));
        memory.write_u16(OAM_BASE + 2, 100 | (1 << 12)); // horizontal flip
        memory.write_u16(OAM_BASE + 4, 0);
        disable_objects_from(&mut memory, 1);
        memory.write_u16(DISPCNT_ADDR, DISPLAY_OBJ_ENABLE | DISPLAY_OBJ_1D_MAPPING);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);
        assert_eq!(pixel(&gpu, 115, 50), [255, 0, 0, 255]);
        assert_eq!(pixel(&gpu, 100, 50), BLACK);

        memory.write_u16(OAM_BASE, 50 | (1 << 14) | (1 << 9)); // disable
        gpu.render(&memory);
        assert_eq!(pixel(&gpu, 115, 50), BLACK);
    }

    #[test]
    fn affine_background_reference_point_advances_per_line() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE + 2, RED);
        // BG2: 128x128 affine map at screen block 8 with tile 1 at map cell (1, 1).
        memory.write_u16(0x0400_000C, 8 << 8);
        memory.vram[0x4000 + 16 + 1] = 1;
        memory.vram[64..128].fill(1); // tile 1 solid, 8bpp
        // PA = 0.5 doubles the width; starting at y = -8 puts map row 1 at lines 16..24.
        memory.write_u16(0x0400_0020, 0x80);
        memory.write_u32(0x0400_002C, (-8i32 << 8) as u32);
        memory.write_u16(DISPCNT_ADDR, 1 | DISPLAY_BG2_ENABLE);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(pixel(&gpu, 16, 15), BLACK);
        assert_eq!(pixel(&gpu, 16, 16), [255, 0, 0, 255]);
        assert_eq!(pixel(&gpu, 31, 23), [255, 0, 0, 255]);
        assert_eq!(pixel(&gpu, 32, 16), BLACK);
        assert_eq!(pixel(&gpu, 16, 24), BLACK);
    }

    #[test]
    fn affine_reference_write_mid_frame_reloads_the_internal_point() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE + 2, RED);
        memory.write_u16(0x0400_000C, 8 << 8);
        memory.vram[0x4000] = 1; // map cell (0, 0) uses tile 1
        memory.vram[64..128].fill(1);
        memory.write_u16(DISPCNT_ADDR, 1 | DISPLAY_BG2_ENABLE);
        let mut gpu = GpuCore::new();

        for line in 0..20 {
            gpu.render_scanline(&memory, line, [false; 2]);
        }
        assert_eq!(pixel(&gpu, 0, 19), BLACK, "line 19 samples map row 2");

        memory.write_u32(0x0400_002C, 0); // BG2Y = 0 again
        gpu.render_scanline(&memory, 20, memory.clone().take_affine_reference_writes());
        assert_eq!(pixel(&gpu, 0, 20), [255, 0, 0, 255]);
    }

    #[test]
    fn window_hides_layers_outside_and_darken_applies_to_targets() {
        let mut memory = MemoryMap::new();
        memory.write_u16(PALETTE_BASE, pack([16, 16, 16])); // gray backdrop
        memory.write_u16(DISPCNT_ADDR, DISPLAY_WIN0_ENABLE);
        memory.write_u16(0x0400_0040, (10 << 8) | 20); // WIN0 x in 10..20
        memory.write_u16(0x0400_0044, 160); // WIN0 y in 0..160
        memory.write_u8(0x0400_0048, WINDOW_EFFECTS); // inside: effects only
        memory.write_u8(0x0400_004A, 0); // outside: no effects
        memory.write_u16(0x0400_0050, (1 << LAYER_BACKDROP) | (3 << 6)); // darken backdrop
        memory.write_u8(0x0400_0054, 8); // EVY = 8/16
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(pixel(&gpu, 15, 0), rgb555_to_rgba(pack([8, 8, 8])));
        assert_eq!(pixel(&gpu, 5, 0), rgb555_to_rgba(pack([16, 16, 16])));
    }

    #[test]
    fn alpha_blend_and_brightness_saturate() {
        assert_eq!(alpha_blend(RED, BLUE, 8, 8), pack([15, 0, 15]));
        assert_eq!(alpha_blend(0x7FFF, 0x7FFF, 16, 16), 0x7FFF);
        assert_eq!(adjust_brightness(0, 16, true), 0x7FFF);
        assert_eq!(adjust_brightness(0x7FFF, 16, false), 0);
    }
}
