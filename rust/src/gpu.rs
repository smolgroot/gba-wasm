pub const SCREEN_WIDTH: usize = 240;
pub const SCREEN_HEIGHT: usize = 160;

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

    pub fn write_test_pattern(&mut self) {
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let index = (y * SCREEN_WIDTH + x) * 4;
                let r = ((x * 255) / SCREEN_WIDTH.max(1)) as u8;
                let g = ((y * 255) / SCREEN_HEIGHT.max(1)) as u8;
                let b = ((x + y) * 255 / (SCREEN_WIDTH + SCREEN_HEIGHT).max(1)) as u8;

                self.framebuffer[index] = r;
                self.framebuffer[index + 1] = g;
                self.framebuffer[index + 2] = b;
                self.framebuffer[index + 3] = 255;
            }
        }
    }
}

impl Default for GpuCore {
    fn default() -> Self {
        Self::new()
    }
}
