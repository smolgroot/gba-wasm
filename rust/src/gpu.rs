pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
const VRAM_BASE: u32 = 0x0600_0000;
const DISPLAY_MODE_MASK: u16 = 0x0007;
const DISPLAY_MODE_3: u16 = 3;
const DISPLAY_MODE_4: u16 = 4;
const DISPLAY_MODE_5: u16 = 5;
const DISPLAY_FRAME_SELECT: u16 = 1 << 4;
const DISPLAY_FORCED_BLANK: u16 = 1 << 7;
const DISPLAY_BG2_ENABLE: u16 = 1 << 10;
const BITMAP_BACK_FRAME_OFFSET: usize = 0xA000;
const MODE5_WIDTH: usize = 160;
const MODE5_HEIGHT: usize = 128;
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn rgb555_to_rgba(color: u16) -> [u8; 4] {
    let expand = |channel: u16| {
        let channel = (channel & 0x1F) as u8;
        (channel << 3) | (channel >> 2)
    };
    [expand(color), expand(color >> 5), expand(color >> 10), 255]
}

fn read_halfword(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

#[derive(Debug, Clone)]
pub struct GpuCore {
    pub framebuffer: Vec<u8>,
}

impl GpuCore {
    pub fn new() -> Self {
        Self {
            framebuffer: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
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

    fn fill(&mut self, rgba: [u8; 4]) {
        for pixel in self.framebuffer.chunks_exact_mut(4) {
            pixel.copy_from_slice(&rgba);
        }
    }

    pub fn render_mode3(&mut self, memory: &crate::memory::MemoryMap) {
        for pixel_index in 0..SCREEN_WIDTH * SCREEN_HEIGHT {
            let address = VRAM_BASE + (pixel_index * 2) as u32;
            let color = memory.read_u16(address);
            let output_index = pixel_index * 4;
            self.framebuffer[output_index..output_index + 4]
                .copy_from_slice(&rgb555_to_rgba(color));
        }
    }

    /// 240x160 8-bit palette indices into the background palette.
    fn render_mode4(&mut self, memory: &crate::memory::MemoryMap, frame_offset: usize) {
        for pixel_index in 0..SCREEN_WIDTH * SCREEN_HEIGHT {
            let palette_index = usize::from(memory.vram[frame_offset + pixel_index]);
            let color = read_halfword(&memory.palette, palette_index * 2);
            let output_index = pixel_index * 4;
            self.framebuffer[output_index..output_index + 4]
                .copy_from_slice(&rgb555_to_rgba(color));
        }
    }

    /// 160x128 RGB555 bitmap; the rest of the screen shows the backdrop.
    fn render_mode5(
        &mut self,
        memory: &crate::memory::MemoryMap,
        frame_offset: usize,
        backdrop: [u8; 4],
    ) {
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let rgba = if x < MODE5_WIDTH && y < MODE5_HEIGHT {
                    let offset = frame_offset + (y * MODE5_WIDTH + x) * 2;
                    rgb555_to_rgba(read_halfword(&memory.vram, offset))
                } else {
                    backdrop
                };
                self.set_pixel(x, y, rgba);
            }
        }
    }

    /// Renders the whole frame from the current display state. Tile modes 0-2, objects,
    /// windows, and blending are not implemented; those frames show only the backdrop.
    pub fn render(&mut self, memory: &crate::memory::MemoryMap) {
        let display_control = memory.read_u16(crate::memory::DISPCNT_ADDR);
        if display_control & DISPLAY_FORCED_BLANK != 0 {
            self.fill(WHITE);
            return;
        }

        let backdrop = rgb555_to_rgba(read_halfword(&memory.palette, 0));
        let frame_offset = if display_control & DISPLAY_FRAME_SELECT != 0 {
            BITMAP_BACK_FRAME_OFFSET
        } else {
            0
        };
        let bg2_enabled = display_control & DISPLAY_BG2_ENABLE != 0;
        match display_control & DISPLAY_MODE_MASK {
            DISPLAY_MODE_3 if bg2_enabled => self.render_mode3(memory),
            DISPLAY_MODE_4 if bg2_enabled => self.render_mode4(memory, frame_offset),
            DISPLAY_MODE_5 if bg2_enabled => self.render_mode5(memory, frame_offset, backdrop),
            _ => self.fill(backdrop),
        }
    }
}

impl Default for GpuCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryMap;

    #[test]
    fn mode3_decodes_rgb555_pixels_from_vram() {
        let mut memory = MemoryMap::new();
        memory.write_u16(
            crate::memory::DISPCNT_ADDR,
            DISPLAY_MODE_3 | DISPLAY_BG2_ENABLE,
        );
        memory.write_u16(VRAM_BASE, 0x001F);
        memory.write_u16(VRAM_BASE + 2, 0x03E0);
        memory.write_u16(VRAM_BASE + 4, 0x7C00);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(&gpu.framebuffer[0..4], &[255, 0, 0, 255]);
        assert_eq!(&gpu.framebuffer[4..8], &[0, 255, 0, 255]);
        assert_eq!(&gpu.framebuffer[8..12], &[0, 0, 255, 255]);
    }

    #[test]
    fn mode4_reads_palette_indices_from_the_selected_frame() {
        let mut memory = MemoryMap::new();
        memory.write_u16(0x0500_0002, 0x001F); // palette 1: red
        memory.write_u16(0x0500_0004, 0x7C00); // palette 2: blue
        memory.vram[0] = 1;
        memory.vram[BITMAP_BACK_FRAME_OFFSET] = 2;
        let mut gpu = GpuCore::new();

        memory.write_u16(
            crate::memory::DISPCNT_ADDR,
            DISPLAY_MODE_4 | DISPLAY_BG2_ENABLE,
        );
        gpu.render(&memory);
        assert_eq!(&gpu.framebuffer[0..4], &[255, 0, 0, 255]);

        memory.write_u16(
            crate::memory::DISPCNT_ADDR,
            DISPLAY_MODE_4 | DISPLAY_BG2_ENABLE | DISPLAY_FRAME_SELECT,
        );
        gpu.render(&memory);
        assert_eq!(&gpu.framebuffer[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn mode5_draws_a_160x128_bitmap_inside_the_backdrop() {
        let mut memory = MemoryMap::new();
        memory.write_u16(0x0500_0000, 0x03E0); // backdrop: green
        memory.write_u16(VRAM_BASE + ((MODE5_WIDTH as u32 - 1) * 2), 0x001F);
        memory.write_u16(
            crate::memory::DISPCNT_ADDR,
            DISPLAY_MODE_5 | DISPLAY_BG2_ENABLE,
        );
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        let pixel = |x: usize, y: usize| &gpu.framebuffer[(y * SCREEN_WIDTH + x) * 4..][..4];
        assert_eq!(pixel(MODE5_WIDTH - 1, 0), &[255, 0, 0, 255]);
        assert_eq!(pixel(MODE5_WIDTH, 0), &[0, 255, 0, 255]);
        assert_eq!(pixel(0, MODE5_HEIGHT), &[0, 255, 0, 255]);
    }

    #[test]
    fn forced_blank_displays_white() {
        let mut memory = MemoryMap::new();
        memory.write_u16(
            crate::memory::DISPCNT_ADDR,
            DISPLAY_MODE_3 | DISPLAY_BG2_ENABLE | DISPLAY_FORCED_BLANK,
        );
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert!(gpu.framebuffer.iter().all(|byte| *byte == 255));
    }

    #[test]
    fn unsupported_display_modes_do_not_render_mode3_vram() {
        let mut memory = MemoryMap::new();
        memory.write_u16(VRAM_BASE, 0x001F);
        let mut gpu = GpuCore::new();

        gpu.render(&memory);

        assert_eq!(&gpu.framebuffer[0..4], &[0, 0, 0, 255]);
    }
}
