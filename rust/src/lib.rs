mod bios;
mod cartridge;
mod cpu;
mod gpu;
mod memory;

pub use cartridge::{CartridgeError, CartridgeHeader, MAX_ROM_SIZE};
pub use cpu::{CpuError, ExecutionState, MAX_FRAME_INSTRUCTIONS, ROM_BASE, ThumbCpu};
pub use gpu::{GpuCore, SCREEN_HEIGHT, SCREEN_WIDTH};
pub use memory::{
    CYCLES_PER_FRAME, CYCLES_PER_SCANLINE, DISPCNT_ADDR, DISPSTAT_ADDR, DMA0_ADDR, IE_ADDR,
    IF_ADDR, IME_ADDR, KEYCNT_ADDR, KEYINPUT_ADDR, MemoryMap, SCANLINES_PER_FRAME, TIMER0_ADDR,
    VCOUNT_ADDR, VISIBLE_SCANLINES, WAITCNT_ADDR,
};

use std::fmt;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GbaError {
    Cartridge(CartridgeError),
    Cpu(CpuError),
}

impl fmt::Display for GbaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cartridge(error) => error.fmt(formatter),
            Self::Cpu(error) => error.fmt(formatter),
        }
    }
}

impl From<CartridgeError> for GbaError {
    fn from(error: CartridgeError) -> Self {
        Self::Cartridge(error)
    }
}

impl From<CpuError> for GbaError {
    fn from(error: CpuError) -> Self {
        Self::Cpu(error)
    }
}

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

    pub fn set_pressed_keys(&mut self, pressed: u16) {
        self.memory.set_pressed_keys(pressed);
    }

    pub fn advance_video_cycles(&mut self, cycles: u32) {
        self.memory.advance_video_cycles(cycles);
    }

    pub fn load_rom(&mut self, rom: &[u8]) -> Result<(), GbaError> {
        CartridgeHeader::parse(rom)?;
        self.memory.load_cartridge(rom);
        self.memory.reset_io_registers();
        self.memory.reset_video_clock();
        self.cpu = ThumbCpu::boot_hle();
        Ok(())
    }

    pub fn step_frame(&mut self) -> Result<Vec<u8>, GbaError> {
        if self.memory.cartridge.is_empty() {
            return Err(CpuError::EmptyCartridge.into());
        }

        // A frame ends when the display enters VBlank, after all 160 visible lines.
        let frame = self.memory.vblanks_started();
        for _ in 0..MAX_FRAME_INSTRUCTIONS {
            self.cpu
                .step_with_cycles(&mut self.memory)
                .map_err(GbaError::Cpu)?;
            if self.memory.vblanks_started() != frame {
                return Ok(self.render_frame());
            }
        }

        Err(CpuError::InstructionBudgetExceeded {
            limit: MAX_FRAME_INSTRUCTIONS,
        }
        .into())
    }

    pub fn render_frame(&mut self) -> Vec<u8> {
        self.gpu.render(&self.memory);
        self.gpu.framebuffer.clone()
    }

    pub fn run_thumb_rom(&mut self, rom: &[u8], steps: u32) -> Result<Vec<u8>, CpuError> {
        self.memory.load_cartridge(rom);
        self.cpu = ThumbCpu::new(ROM_BASE);
        self.cpu.run_steps(&mut self.memory, steps)?;
        Ok(self.render_frame())
    }

    pub fn run_rom(&mut self, rom: &[u8], steps: u32) -> Result<Vec<u8>, GbaError> {
        self.load_rom(rom)?;
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
        core.memory.write_u16(memory::DISPCNT_ADDR, 0x0403);

        let frame = core.run_thumb_rom(&rom, 3).unwrap();

        assert_eq!(&frame[0..4], &[255, 0, 0, 255]);
        assert_eq!(&frame[4..8], &[0, 0, 0, 255]);
    }

    fn generated_arm_mode3_startup_fixture() -> Vec<u8> {
        let mut rom = cartridge::synthetic_rom(*b"FIXT", [0x2E, 0x00, 0x00, 0xEA]);
        let startup: [u32; 8] = [
            0xE3A0_0406, // MOV r0, #0x06000000
            0xE3A0_101F, // MOV r1, #0x1F
            0xE580_1000, // STR r1, [r0]
            0xE3A0_2404, // MOV r2, #0x04000000
            0xE3A0_3003, // MOV r3, #3
            0xE383_3B01, // ORR r3, r3, #0x400 (BG2 enable)
            0xE1C2_30B0, // STRH r3, [r2]
            0xEAFF_FFFE, // B .
        ];
        rom.resize(0xC0 + startup.len() * 4, 0);
        for (index, instruction) in startup.iter().enumerate() {
            let offset = 0xC0 + index * 4;
            rom[offset..offset + 4].copy_from_slice(&instruction.to_le_bytes());
        }
        rom
    }

    #[test]
    fn generated_arm_startup_fixture_enables_mode3_and_reaches_a_known_frame() {
        let mut core = GbaCore::new();

        let frame = core
            .run_rom(&generated_arm_mode3_startup_fixture(), 8)
            .unwrap();

        assert_eq!(core.memory.read_u16(memory::DISPCNT_ADDR), 0x0403);
        assert_eq!(core.cpu.registers[15], ROM_BASE + 0xDC);
        assert_eq!(&frame[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn arm_entry_rom_branches_and_executes_in_hle_boot_mode() {
        let mut rom = cartridge::synthetic_rom(*b"TEST", [0x2E, 0x00, 0x00, 0xEA]);
        rom[0xC0..0xC4].copy_from_slice(&[0x2A, 0x00, 0xA0, 0xE3]);
        let mut core = GbaCore::new();

        core.run_rom(&rom, 2).unwrap();

        assert_eq!(core.cpu.registers[0], 42);
        assert_eq!(core.cpu.registers[15], ROM_BASE + 0xC4);
        assert!(!core.cpu.is_thumb());
    }

    #[test]
    fn step_frame_returns_at_each_vblank_start() {
        let rom = cartridge::synthetic_rom(*b"IDLE", [0xFE, 0xFF, 0xFF, 0xEA]); // B .
        let mut core = GbaCore::new();
        core.load_rom(&rom).unwrap();

        for frame in 1..=3 {
            core.step_frame().unwrap();
            assert_eq!(core.memory.vblanks_started(), frame);
            assert_eq!(core.memory.read_u16(VCOUNT_ADDR), VISIBLE_SCANLINES as u16);
            assert_ne!(core.memory.read_u16(DISPSTAT_ADDR) & 1, 0);
        }
    }

    #[test]
    fn halted_cpu_wakes_on_the_exact_vblank_request() {
        let mut rom = cartridge::synthetic_rom(*b"HALT", [0x2E, 0x00, 0x00, 0xEA]);
        rom.resize(0xC0, 0);
        rom.extend(0xEF02_0000u32.to_le_bytes()); // SWI Halt
        rom.extend(0xEAFF_FFFEu32.to_le_bytes()); // B .
        let mut core = GbaCore::new();
        core.load_rom(&rom).unwrap();
        core.memory.write_u16(DISPSTAT_ADDR, 1 << 3);
        core.memory.write_u16(IE_ADDR, 1);

        core.cpu.run_steps(&mut core.memory, 2).unwrap(); // B to entry; SWI Halt
        let mut halted_steps = 0;
        while core.memory.read_u16(IF_ADDR) == 0 {
            core.cpu.step(&mut core.memory).unwrap();
            halted_steps += 1;
        }

        assert_eq!(core.memory.read_u16(VCOUNT_ADDR), VISIBLE_SCANLINES as u16);
        assert!(
            halted_steps <= 2 * VISIBLE_SCANLINES,
            "halt skipped {halted_steps} times"
        );
        core.cpu.step(&mut core.memory).unwrap(); // resumes at the B . after the SWI
        assert_eq!(core.cpu.registers[15], ROM_BASE + 0xC4);
    }

    #[test]
    fn arm_runner_rejects_invalid_cartridge_header_before_execution() {
        let mut core = GbaCore::new();

        assert!(matches!(
            core.run_rom(&[0; 12], 2),
            Err(GbaError::Cartridge(CartridgeError::TruncatedHeader { .. }))
        ));
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

#[wasm_bindgen]
pub struct WasmEmulator {
    core: GbaCore,
}

#[wasm_bindgen]
impl WasmEmulator {
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8]) -> Result<WasmEmulator, JsValue> {
        let mut core = GbaCore::new();
        core.load_rom(rom)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        Ok(Self { core })
    }

    pub fn step_frame(&mut self) -> Result<Vec<u8>, JsValue> {
        self.core
            .step_frame()
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

#[wasm_bindgen]
pub fn inspect_rom(rom: &[u8]) -> Result<String, JsValue> {
    CartridgeHeader::parse(rom)
        .map(|header| header.summary())
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
