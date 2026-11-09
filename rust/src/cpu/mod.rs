use crate::bios::IRQ_VECTOR;
use crate::memory::{BIOS_LATCH_AFTER_SWI, IME_ADDR, IWRAM_SIZE, MemoryMap};
use crate::state::{State, StateError, StateReader, StateWriter, impl_state};
use std::fmt;

mod alu;
mod arm;
mod bios_hle;
#[cfg(test)]
mod test_util;
mod thumb;

use alu::*;
pub(crate) use bios_hle::{affine_matrix, integer_square_root};

pub const ROM_BASE: u32 = 0x0800_0000;
const EWRAM_BASE: u32 = 0x0200_0000;
pub const MAX_FRAME_INSTRUCTIONS: u32 = 1_000_000;

const CPSR_THUMB: u32 = 1 << 5;
const CPSR_IRQ_DISABLE: u32 = 1 << 7;
const CPSR_USER_MODE: u32 = 0x10;
const CPSR_FIQ_MODE: u32 = 0x11;
const CPSR_IRQ_MODE: u32 = 0x12;
const CPSR_SVC_MODE: u32 = 0x13;
const CPSR_ABORT_MODE: u32 = 0x17;
const CPSR_UNDEFINED_MODE: u32 = 0x1B;
const CPSR_SYSTEM_MODE: u32 = 0x1F;
const CPSR_MODE_MASK: u32 = 0x1F;
const FLAG_N: u32 = 1 << 31;
const FLAG_Z: u32 = 1 << 30;
const FLAG_C: u32 = 1 << 29;
const FLAG_V: u32 = 1 << 28;
const HLE_SYSTEM_STACK: u32 = 0x0300_7F00;
const HLE_IRQ_STACK: u32 = 0x0300_7FA0;
const HLE_SVC_STACK: u32 = 0x0300_7FE0;
/// Interrupt flags the game's IRQ handler reports to `IntrWait` (mirror of `0x03FFFFF8`).
const BIOS_IRQ_FLAGS_ADDR: u32 = 0x0300_7FF8;
const EXCEPTION_ENTRY_CYCLES: u32 = 3;
/// Longest backward branch, in bytes, that may close an idle loop.
const IDLE_LOOP_MAX_SPAN: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionState {
    Arm,
    Thumb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuError {
    EmptyCartridge,
    UnsupportedInstruction {
        address: u32,
        state: ExecutionState,
        opcode: u16,
    },
    UnsupportedArmInstruction {
        address: u32,
        state: ExecutionState,
        opcode: u32,
    },
    UnsupportedBiosCall {
        address: u32,
        state: ExecutionState,
        service: u8,
    },
    InstructionBudgetExceeded {
        limit: u32,
    },
}

impl fmt::Display for CpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCartridge => write!(formatter, "cartridge ROM is empty"),
            Self::UnsupportedInstruction {
                address,
                state,
                opcode,
            } => {
                write!(
                    formatter,
                    "unsupported Thumb opcode 0x{opcode:04X} at 0x{address:08X} in {state:?} state"
                )
            }
            Self::UnsupportedArmInstruction {
                address,
                state,
                opcode,
            } => {
                write!(
                    formatter,
                    "unsupported ARM opcode 0x{opcode:08X} at 0x{address:08X} in {state:?} state"
                )
            }
            Self::UnsupportedBiosCall {
                address,
                state,
                service,
            } => write!(
                formatter,
                "unsupported BIOS SWI 0x{service:02X} at 0x{address:08X} in {state:?} state"
            ),
            Self::InstructionBudgetExceeded { limit } => write!(
                formatter,
                "frame step exceeded its instruction budget of {limit}"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum HaltState {
    /// `SWI Halt`: resume once any enabled interrupt is requested.
    #[default]
    Halt,
    /// `SWI IntrWait`/`VBlankIntrWait`: after waking, re-run the SWI so it can
    /// check the BIOS interrupt flags written by the game's IRQ handler.
    IntrWait { swi_address: u32 },
}

impl State for HaltState {
    fn save(&self, writer: &mut StateWriter) {
        match self {
            Self::Halt => 0u8.save(writer),
            Self::IntrWait { swi_address } => {
                1u8.save(writer);
                swi_address.save(writer);
            }
        }
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut tag = 0u8;
        tag.load(reader)?;
        *self = match tag {
            0 => Self::Halt,
            1 => {
                let mut swi_address = 0;
                swi_address.load(reader)?;
                Self::IntrWait { swi_address }
            }
            _ => return Err(StateError::Invalid("halt state")),
        };
        Ok(())
    }
}

/// A short loop that may be spinning without side effects. Once two consecutive arrivals at
/// `head` see the same registers, with only side-effect-free instructions in between and no
/// reads of continuously changing registers, the loop cannot exit until something outside
/// the CPU changes, so time can jump to the next event.
#[derive(Debug, Clone, Copy)]
struct IdleLoop {
    head: u32,
    registers: [u32; 16],
    cpsr: u32,
    snapshot_valid: bool,
    pure: bool,
}

impl IdleLoop {
    fn new(head: u32) -> Self {
        Self {
            head,
            registers: [0; 16],
            cpsr: 0,
            snapshot_valid: false,
            pure: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThumbCpu {
    pub registers: [u32; 16],
    pub cpsr: u32,
    user_bank: [u32; 7],
    fiq_bank: [u32; 7],
    irq_bank: [u32; 2],
    svc_bank: [u32; 2],
    abort_bank: [u32; 2],
    undefined_bank: [u32; 2],
    /// Saved PSRs for FIQ, IRQ, Supervisor, Abort, and Undefined modes.
    spsr_bank: [u32; 5],
    halt: Option<HaltState>,
    intr_wait_resumed: bool,
    idle_loop: Option<IdleLoop>,
}

// The idle-loop detector is a cache of recent execution; it restarts empty after a load.
impl_state!(ThumbCpu {
    registers, cpsr, user_bank, fiq_bank, irq_bank, svc_bank, abort_bank, undefined_bank,
    spsr_bank, halt, intr_wait_resumed,
} skip { idle_loop });

impl ThumbCpu {
    pub(crate) fn clear_idle_loop(&mut self) {
        self.idle_loop = None;
    }

    pub fn new(entry: u32) -> Self {
        let mut registers = [0; 16];
        registers[15] = entry;
        Self {
            registers,
            cpsr: CPSR_SYSTEM_MODE | CPSR_THUMB,
            user_bank: [0; 7],
            fiq_bank: [0; 7],
            irq_bank: [0; 2],
            svc_bank: [0; 2],
            abort_bank: [0; 2],
            undefined_bank: [0; 2],
            spsr_bank: [0; 5],
            halt: None,
            intr_wait_resumed: false,
            idle_loop: None,
        }
    }

    /// Register state the hardware BIOS leaves behind when it jumps to the cartridge.
    pub fn boot_hle() -> Self {
        let mut cpu = Self::new(ROM_BASE);
        cpu.cpsr = CPSR_SYSTEM_MODE;
        cpu.registers[13] = HLE_SYSTEM_STACK;
        cpu.user_bank[5] = HLE_SYSTEM_STACK;
        cpu.irq_bank[0] = HLE_IRQ_STACK;
        cpu.svc_bank[0] = HLE_SVC_STACK;
        cpu
    }

    pub fn is_thumb(&self) -> bool {
        self.cpsr & CPSR_THUMB != 0
    }

    pub fn run_steps(&mut self, memory: &mut MemoryMap, steps: u32) -> Result<(), CpuError> {
        if memory.cartridge.is_empty() {
            return Err(CpuError::EmptyCartridge);
        }

        for _ in 0..steps {
            self.step(memory)?;
        }
        Ok(())
    }

    pub fn step(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
        self.step_with_cycles(memory).map(|_| ())
    }

    pub fn step_with_cycles(&mut self, memory: &mut MemoryMap) -> Result<u32, CpuError> {
        // DMA owns the bus while it runs, including while the CPU is halted.
        let dma_cycles = memory.run_pending_dma();
        if dma_cycles != 0 {
            self.idle_loop = None;
            memory.advance_video_cycles(dma_cycles);
            return Ok(dma_cycles);
        }

        if let Some(halt) = self.halt {
            if !memory.halt_wake_requested() {
                let idle_cycles = memory.cycles_until_next_event();
                memory.advance_video_cycles(idle_cycles);
                return Ok(idle_cycles);
            }
            self.halt = None;
            if let HaltState::IntrWait { swi_address } = halt {
                self.registers[15] = swi_address;
                self.intr_wait_resumed = true;
            }
        }

        if self.cpsr & CPSR_IRQ_DISABLE == 0 && memory.irq_line_asserted() {
            let return_address = self.registers[15].wrapping_add(4);
            self.enter_exception(CPSR_IRQ_MODE, IRQ_VECTOR, return_address);
            self.idle_loop = None;
            memory.advance_video_cycles(EXCEPTION_ENTRY_CYCLES);
            return Ok(EXCEPTION_ENTRY_CYCLES);
        }

        let faulting_pc = self.registers[15];
        memory.note_opcode_fetch(faulting_pc, self.is_thumb());
        if let Some(idle_cycles) = self.idle_loop_skip(memory, faulting_pc) {
            return Ok(idle_cycles);
        }

        let cycles = self.instruction_cycles(memory);
        let side_effect_free = self.side_effect_free_instruction(memory);
        let result = if self.is_thumb() {
            self.step_thumb(memory)
        } else {
            self.step_arm(memory)
        };
        if result.is_err() {
            self.registers[15] = faulting_pc;
            return result.map(|_| cycles);
        }
        self.track_idle_loop(faulting_pc, side_effect_free);
        memory.advance_video_cycles(cycles);
        Ok(cycles)
    }

    /// At the head of a confirmed idle loop, advances time to the next event instead of
    /// executing. The snapshot is then dropped, so one real iteration runs after every
    /// event and sees any register or memory the event changed.
    fn idle_loop_skip(&mut self, memory: &mut MemoryMap, pc: u32) -> Option<u32> {
        let idle = self.idle_loop.as_mut().filter(|idle| idle.head == pc)?;
        let volatile_read = memory.take_volatile_read();
        let unchanged = idle.snapshot_valid
            && idle.pure
            && !volatile_read
            && idle.registers == self.registers
            && idle.cpsr == self.cpsr;
        if unchanged {
            idle.snapshot_valid = false;
            let cycles = memory.cycles_until_next_event();
            memory.advance_video_cycles(cycles);
            return Some(cycles);
        }
        *idle = IdleLoop {
            registers: self.registers,
            cpsr: self.cpsr,
            snapshot_valid: true,
            ..IdleLoop::new(pc)
        };
        None
    }

    fn track_idle_loop(&mut self, address: u32, side_effect_free: bool) {
        if !side_effect_free {
            if let Some(idle) = self.idle_loop.as_mut() {
                idle.pure = false;
            }
            return;
        }
        let target = self.registers[15];
        let short_backward_branch = target < address && address - target <= IDLE_LOOP_MAX_SPAN;
        if short_backward_branch && self.idle_loop.is_none_or(|idle| idle.head != target) {
            self.idle_loop = Some(IdleLoop::new(target));
        }
    }

    /// Instructions that only change registers and flags: ALU operations, loads, and branches
    /// without link. Stores, block transfers, PC writes other than branches, PSR transfers,
    /// SWIs, and calls are excluded.
    fn side_effect_free_instruction(&self, memory: &MemoryMap) -> bool {
        if self.is_thumb() {
            let opcode = memory.read_u16(self.registers[15] & !1);
            return match opcode >> 11 {
                0x00..=0x07 | 0x09 | 0x0D | 0x0F | 0x11 | 0x13 | 0x14 | 0x15 | 0x1C => true,
                0x08 if opcode & 0x0400 == 0 => true, // ALU operations
                0x08 => {
                    let destination = (opcode & 0x7) | ((opcode >> 4) & 0x8);
                    (opcode >> 8) & 3 != 3 && destination != 15
                }
                0x0A | 0x0B => (opcode >> 9) & 7 >= 3, // register-offset loads
                0x1A | 0x1B => (opcode >> 8) & 0xF < 0xE, // conditional branches, not SWI
                _ => false,
            };
        }

        let opcode = memory.read_u32(self.registers[15] & !3);
        let destination = (opcode >> 12) & 0xF;
        if opcode >> 28 == 0xF {
            return false;
        }
        match (opcode >> 25) & 7 {
            0b101 => opcode & (1 << 24) == 0, // B, not BL
            0b010 | 0b011 => opcode & (1 << 20) != 0 && destination != 15, // LDR/LDRB
            0b000 if opcode & 0x90 == 0x90 => {
                // Multiply, or a halfword/signed load; not stores or SWP.
                let multiply = opcode & 0x60 == 0 && opcode & 0x0100_0000 == 0;
                let halfword_load = opcode & 0x60 != 0 && opcode & (1 << 20) != 0;
                (multiply || halfword_load) && destination != 15
            }
            0b000 | 0b001 => {
                let psr_transfer = opcode & 0x0190_0000 == 0x0100_0000;
                !psr_transfer && destination != 15
            }
            _ => false,
        }
    }

    fn instruction_cycles(&self, memory: &MemoryMap) -> u32 {
        if self.is_thumb() {
            let address = self.registers[15] & !1;
            let opcode = memory.read_u16(address);
            return self.thumb_instruction_cycles(opcode);
        }

        let address = self.registers[15] & !3;
        let opcode = memory.read_u32(address);
        let condition = (opcode >> 28) as u8;
        if condition == 0xF || !self.condition_passed(condition) {
            return 1;
        }

        if opcode & 0x0FF0_00F0 == 0x0120_0010 || opcode & 0x0E00_0000 == 0x0A00_0000 {
            return 3;
        }
        if opcode & 0x0F00_0000 == 0x0F00_0000 {
            return self.bios_instruction_cycles((opcode >> 16) & 0xFF);
        }
        if opcode & 0x0E00_0000 == 0x0800_0000 {
            let count = (opcode & 0xFFFF).count_ones();
            return count + if opcode & (1 << 20) != 0 { 2 } else { 1 };
        }
        if opcode & 0x0FC0_00F0 == 0x0000_0090 {
            let multiplier = self.registers[((opcode >> 8) & 0xF) as usize];
            return 1 + multiply_array_cycles(multiplier, true) + ((opcode >> 21) & 1);
        }
        if opcode & 0x0F80_00F0 == 0x0080_0090 {
            let multiplier = self.registers[((opcode >> 8) & 0xF) as usize];
            let signed = opcode & (1 << 22) != 0;
            return 2 + multiply_array_cycles(multiplier, signed) + ((opcode >> 21) & 1);
        }
        if opcode & 0x0FB0_00F0 == 0x0100_0090 {
            return 4;
        }
        if opcode & 0x0E00_0090 == 0x0000_0090 {
            return if opcode & (1 << 20) != 0 { 3 } else { 2 };
        }
        if opcode & 0x0C00_0000 == 0x0400_0000 {
            let is_load = opcode & (1 << 20) != 0;
            let loads_pc = opcode & 0x0000_F000 == 0x0000_F000 && is_load;
            return if is_load {
                3 + u32::from(loads_pc) * 2
            } else {
                2
            };
        }
        if opcode & 0x0C00_0000 == 0x0000_0000 {
            let operation = (opcode >> 21) & 0xF;
            let writes_result = !matches!(operation, 8..=11);
            let destination = (opcode >> 12) & 0xF;
            let register_shift = opcode & (1 << 25) == 0 && opcode & (1 << 4) != 0;
            let pc_write = writes_result && destination == 15;
            return 1 + u32::from(register_shift) + u32::from(pc_write) * 2;
        }
        1
    }

    fn thumb_instruction_cycles(&self, opcode: u16) -> u32 {
        if opcode & 0xF000 == 0xD000 {
            let condition = ((opcode >> 8) & 0xF) as u8;
            if condition == 0xF {
                return self.bios_instruction_cycles(u32::from(opcode & 0xFF));
            }
            if condition < 0xE && self.condition_passed(condition) {
                return 3;
            }
            return 1;
        }
        if opcode & 0xF800 == 0xE000 || opcode & 0xF800 == 0xF800 {
            return 3;
        }
        if opcode & 0xF800 == 0xF000 {
            return 1;
        }
        if opcode & 0xFE00 == 0xB400 {
            return 1 + (opcode & 0x00FF).count_ones() + u32::from(opcode & 0x0100 != 0);
        }
        if opcode & 0xFE00 == 0xBC00 {
            let pops_pc = opcode & 0x0100 != 0;
            return 2 + (opcode & 0x00FF).count_ones() + u32::from(pops_pc) * 3;
        }
        if opcode & 0xF000 == 0xC000 {
            return 1 + (opcode & 0x00FF).count_ones();
        }
        if opcode & 0xFFC0 == 0x4340 {
            let multiplier = self.registers[usize::from(opcode & 0x7)];
            return 1 + multiply_array_cycles(multiplier, true);
        }
        if matches!(opcode & 0xF000, 0x5000 | 0x6000 | 0x7000 | 0x8000 | 0x9000) {
            let is_load = if opcode & 0xF000 == 0x5000 {
                opcode & 0x0E00 >= 0x0600 || opcode & 0x0E00 == 0x0A00
            } else {
                opcode & 0x0800 != 0
            };
            return if is_load { 3 } else { 2 };
        }
        if opcode & 0xF800 == 0x4800 {
            return 3;
        }
        if opcode & 0xFC00 == 0x4400 {
            let operation = (opcode >> 8) & 0x3;
            if operation == 3 {
                return 3;
            }
            if operation != 1 {
                let destination = usize::from((opcode & 0x7) | ((opcode >> 4) & 0x8));
                if destination == 15 {
                    return 3;
                }
            }
        }
        if opcode & 0xFC00 == 0x4000 && matches!((opcode >> 6) & 0xF, 2 | 3 | 4 | 7) {
            return 2;
        }
        1
    }

    fn condition_passed(&self, condition: u8) -> bool {
        let negative = self.cpsr & FLAG_N != 0;
        let zero = self.cpsr & FLAG_Z != 0;
        let carry = self.cpsr & FLAG_C != 0;
        let overflow = self.cpsr & FLAG_V != 0;

        match condition {
            0x0 => zero,
            0x1 => !zero,
            0x2 => carry,
            0x3 => !carry,
            0x4 => negative,
            0x5 => !negative,
            0x6 => overflow,
            0x7 => !overflow,
            0x8 => carry && !zero,
            0x9 => !carry || zero,
            0xA => negative == overflow,
            0xB => negative != overflow,
            0xC => !zero && negative == overflow,
            0xD => zero || negative != overflow,
            0xE => true,
            _ => false,
        }
    }

    fn read_register(
        &self,
        register: usize,
        state: ExecutionState,
        instruction_address: u32,
    ) -> u32 {
        if register != 15 {
            return self.registers[register];
        }

        match state {
            ExecutionState::Arm => instruction_address.wrapping_add(8),
            ExecutionState::Thumb => instruction_address.wrapping_add(4),
        }
    }

    fn is_valid_mode(mode: u32) -> bool {
        matches!(
            mode,
            CPSR_USER_MODE
                | CPSR_FIQ_MODE
                | CPSR_IRQ_MODE
                | CPSR_SVC_MODE
                | CPSR_ABORT_MODE
                | CPSR_UNDEFINED_MODE
                | CPSR_SYSTEM_MODE
        )
    }

    fn switch_mode(&mut self, new_mode: u32) {
        let old_mode = self.cpsr & CPSR_MODE_MASK;
        if old_mode == new_mode {
            return;
        }

        self.save_active_bank(old_mode);
        self.load_active_bank(new_mode);
    }

    fn save_active_bank(&mut self, mode: u32) {
        if mode == CPSR_FIQ_MODE {
            self.fiq_bank.copy_from_slice(&self.registers[8..15]);
            return;
        }

        self.user_bank[..5].copy_from_slice(&self.registers[8..13]);
        match mode {
            CPSR_IRQ_MODE => self.irq_bank.copy_from_slice(&self.registers[13..15]),
            CPSR_SVC_MODE => self.svc_bank.copy_from_slice(&self.registers[13..15]),
            CPSR_ABORT_MODE => self.abort_bank.copy_from_slice(&self.registers[13..15]),
            CPSR_UNDEFINED_MODE => self.undefined_bank.copy_from_slice(&self.registers[13..15]),
            _ => self.user_bank[5..].copy_from_slice(&self.registers[13..15]),
        }
    }

    fn load_active_bank(&mut self, mode: u32) {
        if mode == CPSR_FIQ_MODE {
            self.registers[8..15].copy_from_slice(&self.fiq_bank);
            return;
        }

        self.registers[8..13].copy_from_slice(&self.user_bank[..5]);
        let bank = match mode {
            CPSR_IRQ_MODE => &self.irq_bank,
            CPSR_SVC_MODE => &self.svc_bank,
            CPSR_ABORT_MODE => &self.abort_bank,
            CPSR_UNDEFINED_MODE => &self.undefined_bank,
            _ => &self.user_bank[5..],
        };
        self.registers[13..15].copy_from_slice(bank);
    }

    fn spsr_index(mode: u32) -> Option<usize> {
        match mode {
            CPSR_FIQ_MODE => Some(0),
            CPSR_IRQ_MODE => Some(1),
            CPSR_SVC_MODE => Some(2),
            CPSR_ABORT_MODE => Some(3),
            CPSR_UNDEFINED_MODE => Some(4),
            _ => None,
        }
    }

    fn current_spsr_index(&self) -> Option<usize> {
        Self::spsr_index(self.cpsr & CPSR_MODE_MASK)
    }

    /// Exception return (`MOVS pc, ...` / `LDM {..., pc}^`). User and System modes have no
    /// SPSR; ARM7TDMI then leaves CPSR unchanged.
    fn restore_cpsr_from_spsr(&mut self) {
        let Some(index) = self.current_spsr_index() else {
            return;
        };
        let spsr = self.spsr_bank[index];
        self.switch_mode(spsr & CPSR_MODE_MASK);
        self.cpsr = spsr;
    }

    fn enter_exception(&mut self, mode: u32, vector: u32, return_address: u32) {
        let saved_cpsr = self.cpsr;
        self.switch_mode(mode);
        self.cpsr = (saved_cpsr & !(CPSR_MODE_MASK | CPSR_THUMB)) | mode | CPSR_IRQ_DISABLE;
        if let Some(index) = Self::spsr_index(mode) {
            self.spsr_bank[index] = saved_cpsr;
        }
        self.registers[14] = return_address;
        self.registers[15] = vector;
    }

    /// Reads a User-bank register for `STM ...^`.
    fn read_user_register(&self, register: usize) -> u32 {
        let mode = self.cpsr & CPSR_MODE_MASK;
        match register {
            8..=12 if mode == CPSR_FIQ_MODE => self.user_bank[register - 8],
            13 | 14 if !matches!(mode, CPSR_USER_MODE | CPSR_SYSTEM_MODE) => {
                self.user_bank[register - 8]
            }
            _ => self.registers[register],
        }
    }

    /// Writes a User-bank register for `LDM ...^` without a PC load.
    fn write_user_register(&mut self, register: usize, value: u32) {
        let mode = self.cpsr & CPSR_MODE_MASK;
        match register {
            8..=12 if mode == CPSR_FIQ_MODE => self.user_bank[register - 8] = value,
            13 | 14 if !matches!(mode, CPSR_USER_MODE | CPSR_SYSTEM_MODE) => {
                self.user_bank[register - 8] = value
            }
            _ => self.registers[register] = value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_util::*;

    #[test]
    fn irq_entry_banks_registers_and_subs_pc_lr_returns_to_the_interrupted_thumb_code() {
        let mut memory = MemoryMap::new();
        let mut rom = program(&[0x2001, 0x2102]); // MOVS r0, #1; MOVS r1, #2
        rom.resize(0x40, 0);
        rom.extend(0xE12F_FF1Eu32.to_le_bytes()); // handler: BX lr
        memory.load_cartridge(&rom);
        memory.write_u32(0x0300_7FFC, ROM_BASE + 0x40);
        memory.write_u16(crate::memory::IE_ADDR, 1 << 3);
        memory.write_u8(IME_ADDR, 1);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= CPSR_THUMB;
        cpu.step(&mut memory).unwrap();

        memory.io[0x202] = 1 << 3; // timer 0 request
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_IRQ_MODE);
        assert_ne!(cpu.cpsr & CPSR_IRQ_DISABLE, 0);
        assert!(!cpu.is_thumb());
        assert_eq!(cpu.registers[15], IRQ_VECTOR);
        assert_eq!(cpu.registers[14], ROM_BASE + 2 + 4);
        assert_eq!(cpu.registers[13], HLE_IRQ_STACK);

        memory.io[0x202] = 0; // the handler would acknowledge IF
        // B dispatcher; STMFD; MOV; ADD; LDR pc; BX lr (handler); LDMFD; SUBS pc, lr, #4
        for _ in 0..8 {
            cpu.step(&mut memory).unwrap();
        }
        assert_eq!(cpu.registers[15], ROM_BASE + 2);
        assert!(cpu.is_thumb());
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SYSTEM_MODE);
        assert_eq!(cpu.registers[13], HLE_SYSTEM_STACK);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[1], 2);
    }

    #[test]
    fn every_arm_condition_matches_all_nzcv_combinations() {
        let expected: [fn(bool, bool, bool, bool) -> bool; 16] = [
            |_n, z, _c, _v| z,
            |_n, z, _c, _v| !z,
            |_n, _z, c, _v| c,
            |_n, _z, c, _v| !c,
            |n, _z, _c, _v| n,
            |n, _z, _c, _v| !n,
            |_n, _z, _c, v| v,
            |_n, _z, _c, v| !v,
            |_n, z, c, _v| c && !z,
            |_n, z, c, _v| !c || z,
            |n, _z, _c, v| n == v,
            |n, _z, _c, v| n != v,
            |n, z, _c, v| !z && n == v,
            |n, z, _c, v| z || n != v,
            |_n, _z, _c, _v| true,
            |_n, _z, _c, _v| false,
        ];

        for flags in 0..16 {
            let n = flags & 0b1000 != 0;
            let z = flags & 0b0100 != 0;
            let c = flags & 0b0010 != 0;
            let v = flags & 0b0001 != 0;
            let mut cpu = ThumbCpu::boot_hle();
            cpu.cpsr = CPSR_SYSTEM_MODE
                | if n { FLAG_N } else { 0 }
                | if z { FLAG_Z } else { 0 }
                | if c { FLAG_C } else { 0 }
                | if v { FLAG_V } else { 0 };

            for (condition, predicate) in expected.iter().enumerate() {
                assert_eq!(
                    cpu.condition_passed(condition as u8),
                    predicate(n, z, c, v),
                    "condition {condition:X} with NZCV={flags:04b}"
                );
            }
        }
    }

    #[test]
    fn instruction_fetch_aligns_arm_and_thumb_pcs() {
        let mut arm_memory = MemoryMap::new();
        arm_memory.load_cartridge(&[0x2A, 0x00, 0xA0, 0xE3]); // MOV r0, #42
        let mut arm_cpu = ThumbCpu::boot_hle();
        arm_cpu.registers[15] = ROM_BASE + 3;

        arm_cpu.step(&mut arm_memory).unwrap();

        assert_eq!(arm_cpu.registers[0], 42);
        assert_eq!(arm_cpu.registers[15], ROM_BASE + 4);

        let mut thumb_memory = MemoryMap::new();
        thumb_memory.load_cartridge(&program(&[0x202A])); // MOVS r0, #42
        let mut thumb_cpu = ThumbCpu::new(ROM_BASE + 1);

        thumb_cpu.step(&mut thumb_memory).unwrap();

        assert_eq!(thumb_cpu.registers[0], 42);
        assert_eq!(thumb_cpu.registers[15], ROM_BASE + 2);
    }

    #[test]
    fn hle_boot_starts_in_arm_system_mode_with_iwram_stack() {
        let cpu = ThumbCpu::boot_hle();

        assert!(!cpu.is_thumb());
        assert_eq!(cpu.registers[13], 0x0300_7F00);
        assert_eq!(cpu.registers[15], ROM_BASE);
        assert_eq!(cpu.cpsr & 0x1F, CPSR_SYSTEM_MODE);
    }
}
