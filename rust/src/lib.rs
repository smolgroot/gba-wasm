mod cpu;
mod gpu;
mod memory;

pub use cpu::{CpuError, ROM_BASE, ThumbCpu};
pub use gpu::{GpuCore, SCREEN_HEIGHT, SCREEN_WIDTH};
pub use memory::MemoryMap;

use wasm_bindgen::prelude::*;

#[derive(Debug, Clone)]
pub struct GbaCore {
    pub memory: MemoryMap,
    pub gpu: GpuCore,
    pub cpu: ThumbCpu,
}

impl GbaCore {
    pub fn new() -> Self {
        Self {
            memory: MemoryMap::new(),
            gpu: GpuCore::new(),
            cpu: ThumbCpu::new(ROM_BASE),
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

    pub fn run_thumb_rom(&mut self, rom: &[u8], steps: u32) -> Result<Vec<u8>, CpuError> {
        self.memory.load_cartridge(rom);
        self.cpu = ThumbCpu::new(ROM_BASE);
        self.cpu.run_steps(&mut self.memory, steps)?;
        Ok(self.render_frame())
    }

    pub fn run_rom(&mut self, rom: &[u8], steps: u32) -> Result<Vec<u8>, CpuError> {
        self.memory.load_cartridge(rom);
        self.cpu = ThumbCpu::boot_hle();
        self.cpu.run_steps(&mut self.memory, steps)?;
        Ok(self.render_frame())
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

    #[test]
    fn thumb_rom_writes_vram_and_produces_a_mode3_frame() {
        let rom = [
            0x01, 0x48, 0x1F, 0x21, 0x01, 0x60, 0xFE, 0xE7, 0x00, 0x00, 0x00, 0x06,
        ];
        let mut core = GbaCore::new();

        let frame = core.run_thumb_rom(&rom, 3).unwrap();

        assert_eq!(&frame[0..4], &[255, 0, 0, 255]);
        assert_eq!(&frame[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn arm_entry_rom_branches_and_executes_in_hle_boot_mode() {
        let rom = [
            0x00, 0x00, 0x00, 0xEA, // B to ROM_BASE + 8
            0x00, 0x00, 0xA0, 0xE1, // skipped instruction
            0x2A, 0x00, 0xA0, 0xE3, // MOV r0, #42
        ];
        let mut core = GbaCore::new();

        core.run_rom(&rom, 2).unwrap();

        assert_eq!(core.cpu.registers[0], 42);
        assert_eq!(core.cpu.registers[15], ROM_BASE + 12);
        assert!(!core.cpu.is_thumb());
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
pub fn run_thumb_rom(rom: &[u8], steps: u32) -> Result<Vec<u8>, JsValue> {
    if steps > 1_000_000 {
        return Err(JsValue::from_str("step limit must be at most 1,000,000"));
    }

    let mut core = GbaCore::new();
    core.run_thumb_rom(rom, steps)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn run_rom(rom: &[u8], steps: u32) -> Result<Vec<u8>, JsValue> {
    if steps > 1_000_000 {
        return Err(JsValue::from_str("step limit must be at most 1,000,000"));
    }

    let mut core = GbaCore::new();
    core.run_rom(rom, steps)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
