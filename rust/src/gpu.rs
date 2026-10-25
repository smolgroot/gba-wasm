pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;
const VRAM_BASE: u32 = 0x0600_0000;

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

    pub fn render_mode3(&mut self, memory: &crate::memory::MemoryMap) {
        for pixel_index in 0..SCREEN_WIDTH * SCREEN_HEIGHT {
            let address = VRAM_BASE + (pixel_index * 2) as u32;
            let color = memory.read_u16(address);
            let red = (color & 0x1F) as u8;
            let green = ((color >> 5) & 0x1F) as u8;
            let blue = ((color >> 10) & 0x1F) as u8;
            let output_index = pixel_index * 4;

            self.framebuffer[output_index] = (red << 3) | (red >> 2);
            self.framebuffer[output_index + 1] = (green << 3) | (green >> 2);
            self.framebuffer[output_index + 2] = (blue << 3) | (blue >> 2);
            self.framebuffer[output_index + 3] = 255;
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
        memory.write_u16(VRAM_BASE, 0x001F);
        memory.write_u16(VRAM_BASE + 2, 0x03E0);
        memory.write_u16(VRAM_BASE + 4, 0x7C00);
        let mut gpu = GpuCore::new();

        gpu.render_mode3(&memory);

        assert_eq!(&gpu.framebuffer[0..4], &[255, 0, 0, 255]);
        assert_eq!(&gpu.framebuffer[4..8], &[0, 255, 0, 255]);
        assert_eq!(&gpu.framebuffer[8..12], &[0, 0, 255, 255]);
    }
}
