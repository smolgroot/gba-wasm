mod gpu;
mod memory;

pub use gpu::{GpuCore, SCREEN_HEIGHT, SCREEN_WIDTH};
pub use memory::MemoryMap;

use wasm_bindgen::prelude::*;

#[derive(Debug, Clone)]
pub struct GbaCore {
    pub memory: MemoryMap,
    pub gpu: GpuCore,
}

impl GbaCore {
    pub fn new() -> Self {
        let mut gpu = GpuCore::new();
        gpu.write_test_pattern();

        Self {
            memory: MemoryMap::new(),
            gpu,
        }
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        self.memory.read_u8(addr)
    }

    pub fn write_u8(&mut self, addr: u32, value: u8) {
        self.memory.write_u8(addr, value)
    }

    pub fn render_test_frame(&mut self, width: usize, height: usize) -> Vec<u8> {
        let mut output = vec![0u8; width * height * 4];

        let render_width = width.min(SCREEN_WIDTH);
        let render_height = height.min(SCREEN_HEIGHT);

        for y in 0..render_height {
            for x in 0..render_width {
                let source_index = (y * SCREEN_WIDTH + x) * 4;
                let dest_index = (y * width + x) * 4;
                output[dest_index..dest_index + 4]
                    .copy_from_slice(&self.gpu.framebuffer[source_index..source_index + 4]);
            }
        }

        output
    }
}

impl Default for GbaCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_map_reads_and_writes_ewram() {
        let mut core = GbaCore::new();
        core.write_u8(0x0200_0000, 0xAB);

        assert_eq!(core.read_u8(0x0200_0000), 0xAB);
    }

    #[test]
    fn gpu_can_clear_and_draw_pixels() {
        let mut core = GbaCore::new();

        core.gpu.clear();
        core.gpu.set_pixel(10, 20, [255, 0, 0, 255]);

        assert_eq!(core.gpu.framebuffer[(20 * SCREEN_WIDTH + 10) * 4], 255);
        assert_eq!(core.gpu.framebuffer[(20 * SCREEN_WIDTH + 10) * 4 + 1], 0);
        assert_eq!(core.gpu.framebuffer[(20 * SCREEN_WIDTH + 10) * 4 + 2], 0);
        assert_eq!(core.gpu.framebuffer[(20 * SCREEN_WIDTH + 10) * 4 + 3], 255);
    }
}

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn hello(name: &str) -> String {
    format!("GBA WASM ready: {}", name)
}

#[wasm_bindgen]
pub fn make_test_frame(width: usize, height: usize) -> Vec<u8> {
    let mut core = GbaCore::new();
    core.render_test_frame(width, height)
}
