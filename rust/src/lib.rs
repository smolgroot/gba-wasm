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
        Self {
            memory: MemoryMap::new(),
            gpu: GpuCore::new(),
        }
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        self.memory.read_u8(addr)
    }

    pub fn write_u8(&mut self, addr: u32, value: u8) {
        self.memory.write_u8(addr, value)
    }

    pub fn render_frame(&mut self) -> Vec<u8> {
        self.gpu.render_mode3(&self.memory);
        self.gpu.framebuffer.clone()
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
pub fn render_mode3(vram: &[u8]) -> Vec<u8> {
    let mut core = GbaCore::new();
    let copy_len = vram.len().min(core.memory.vram.len());
    core.memory.vram[..copy_len].copy_from_slice(&vram[..copy_len]);
    core.render_frame()
}
