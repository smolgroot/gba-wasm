use crate::bios::IRQ_VECTOR;
use crate::memory::{BIOS_LATCH_AFTER_SWI, IME_ADDR, MemoryMap};
use crate::state::{State, StateError, StateReader, StateWriter, impl_state};
use std::fmt;

pub const ROM_BASE: u32 = 0x0800_0000;
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

        if opcode & 0x0FFF_FFF0 == 0x012F_FF10 || opcode & 0x0E00_0000 == 0x0A00_0000 {
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
        if opcode & 0x0FB0_0FF0 == 0x0100_0090 {
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

    fn bios_instruction_cycles(&self, service: u32) -> u32 {
        match service {
            0x0B => (self.registers[2] & 0x001F_FFFF)
                .saturating_mul(2)
                .saturating_add(3),
            0x0C => (((self.registers[2] & 0x001F_FFFF) + 7) & !7)
                .saturating_mul(2)
                .saturating_add(3),
            _ => 3,
        }
    }

    fn unsupported_arm(address: u32, opcode: u32) -> CpuError {
        CpuError::UnsupportedArmInstruction {
            address,
            state: ExecutionState::Arm,
            opcode,
        }
    }

    fn step_arm(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
        let address = self.registers[15] & !3;
        let opcode = memory.read_u32(address);
        self.registers[15] = address.wrapping_add(4);
        let condition = (opcode >> 28) as u8;
        if condition == 0xF {
            return Err(Self::unsupported_arm(address, opcode));
        }
        if !self.condition_passed(condition) {
            return Ok(());
        }

        if opcode & 0x0FFF_FFF0 == 0x012F_FF10 {
            let target = self.read_register((opcode & 0xF) as usize, ExecutionState::Arm, address);
            self.branch_exchange(target);
            return Ok(());
        }
        if opcode & 0x0F00_0000 == 0x0F00_0000 {
            let service = ((opcode >> 16) & 0xFF) as u8;
            return self.execute_bios_call(memory, address, ExecutionState::Arm, service);
        }
        if opcode & 0x0FC0_00F0 == 0x0000_0090 {
            return self.arm_multiply(opcode, address);
        }
        if opcode & 0x0F80_00F0 == 0x0080_0090 {
            return self.arm_multiply_long(opcode, address);
        }
        if opcode & 0x0FB0_0FF0 == 0x0100_0090 {
            return self.arm_swap(memory, opcode, address);
        }
        if opcode & 0x0E00_0090 == 0x0000_0090 && opcode & 0x60 != 0 {
            return self.arm_halfword_transfer(memory, opcode, address);
        }
        if opcode & 0x0FBF_0FFF == 0x010F_0000 {
            return self.arm_mrs(opcode, address);
        }
        if opcode & 0x0FB0_FFF0 == 0x0120_F000 || opcode & 0x0FB0_F000 == 0x0320_F000 {
            return self.arm_msr(opcode, address);
        }
        if opcode & 0x0C00_0000 == 0 {
            return self.arm_data_processing(opcode, address);
        }
        if opcode & 0x0C00_0000 == 0x0400_0000 {
            return self.arm_single_transfer(memory, opcode, address);
        }
        if opcode & 0x0E00_0000 == 0x0800_0000 {
            return self.arm_block_transfer(memory, opcode, address);
        }
        if opcode & 0x0E00_0000 == 0x0A00_0000 {
            if opcode & (1 << 24) != 0 {
                self.registers[14] = address.wrapping_add(4);
            }
            let offset = sign_extend(opcode & 0x00FF_FFFF, 24) << 2;
            self.registers[15] = address.wrapping_add(8).wrapping_add(offset);
            return Ok(());
        }

        Err(Self::unsupported_arm(address, opcode))
    }

    fn arm_data_processing(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
        let operation = (opcode >> 21) & 0xF;
        let set_flags = opcode & (1 << 20) != 0;
        let destination = ((opcode >> 12) & 0xF) as usize;
        let source_register = ((opcode >> 16) & 0xF) as usize;
        let is_test = matches!(operation, 8..=11);
        let immediate = opcode & (1 << 25) != 0;
        let register_shift = !immediate && opcode & (1 << 4) != 0;

        // Test operations without S are the PSR transfer space, and register operands with
        // bits 7 and 4 set are the multiply/transfer extension space; both are undecoded here.
        if (is_test && !set_flags) || (register_shift && opcode & (1 << 7) != 0) {
            return Err(Self::unsupported_arm(address, opcode));
        }
        // With Rd = PC and S set, CPSR is restored from SPSR. ARM7TDMI does this even for the
        // test operations, which still leave PC (and the pipeline) untouched.
        let restores_cpsr = set_flags && destination == 15;

        // A register-specified shift takes an extra internal cycle, so PC reads as +12.
        let pc_value = address.wrapping_add(if register_shift { 12 } else { 8 });
        let read = |cpu: &Self, register: usize| {
            if register == 15 {
                pc_value
            } else {
                cpu.registers[register]
            }
        };
        let carry_in = self.cpsr & FLAG_C != 0;
        let (operand, shifter_carry) = if immediate {
            let rotation = ((opcode >> 8) & 0xF) * 2;
            let operand = (opcode & 0xFF).rotate_right(rotation);
            let carry = if rotation == 0 {
                carry_in
            } else {
                operand & FLAG_N != 0
            };
            (operand, carry)
        } else {
            let value = read(self, (opcode & 0xF) as usize);
            let shift_type = (opcode >> 5) & 0x3;
            if register_shift {
                let amount = self.registers[((opcode >> 8) & 0xF) as usize];
                shift_register(value, amount, shift_type, carry_in)
            } else {
                shift_immediate(value, shift_type, (opcode >> 7) & 0x1F, carry_in)
            }
        };
        let source = read(self, source_register);
        let carry_in = u32::from(carry_in);

        let (result, arithmetic_flags) = match operation {
            0 | 8 => (source & operand, None),
            1 | 9 => (source ^ operand, None),
            2 | 10 => (
                source.wrapping_sub(operand),
                Some((true, source, operand, 0)),
            ),
            3 => (
                operand.wrapping_sub(source),
                Some((true, operand, source, 0)),
            ),
            4 | 11 => (
                source.wrapping_add(operand),
                Some((false, source, operand, 0)),
            ),
            5 => (
                source.wrapping_add(operand).wrapping_add(carry_in),
                Some((false, source, operand, carry_in)),
            ),
            6 => {
                let borrow = 1 - carry_in;
                (
                    source.wrapping_sub(operand).wrapping_sub(borrow),
                    Some((true, source, operand, borrow)),
                )
            }
            7 => {
                let borrow = 1 - carry_in;
                (
                    operand.wrapping_sub(source).wrapping_sub(borrow),
                    Some((true, operand, source, borrow)),
                )
            }
            12 => (source | operand, None),
            13 => (operand, None),
            14 => (source & !operand, None),
            15 => (!operand, None),
            _ => unreachable!(),
        };

        if restores_cpsr {
            self.restore_cpsr_from_spsr();
        } else if set_flags {
            if let Some((subtract, left, right, adjustment)) = arithmetic_flags {
                self.set_arithmetic_flags(subtract, left, right, adjustment, result);
            } else {
                self.set_nz(result);
                self.set_carry(shifter_carry);
            }
        }

        if !is_test {
            self.write_register_or_pc(destination, result);
        }
        Ok(())
    }

    fn arm_multiply(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
        let accumulate = opcode & (1 << 21) != 0;
        let set_flags = opcode & (1 << 20) != 0;
        let destination = ((opcode >> 16) & 0xF) as usize;
        let operands = [
            (opcode & 0xF) as usize,
            ((opcode >> 8) & 0xF) as usize,
            ((opcode >> 12) & 0xF) as usize,
        ];
        if destination == 15 || operands.contains(&15) {
            return Err(Self::unsupported_arm(address, opcode));
        }

        let mut result = self.registers[operands[0]].wrapping_mul(self.registers[operands[1]]);
        if accumulate {
            result = result.wrapping_add(self.registers[operands[2]]);
        }
        if set_flags {
            // ARM7TDMI leaves C in an implementation-defined state; it is preserved here.
            self.set_nz(result);
        }
        self.registers[destination] = result;
        Ok(())
    }

    fn arm_multiply_long(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
        let signed = opcode & (1 << 22) != 0;
        let accumulate = opcode & (1 << 21) != 0;
        let set_flags = opcode & (1 << 20) != 0;
        let high = ((opcode >> 16) & 0xF) as usize;
        let low = ((opcode >> 12) & 0xF) as usize;
        let multiplicand = (opcode & 0xF) as usize;
        let multiplier = ((opcode >> 8) & 0xF) as usize;
        if [high, low, multiplicand, multiplier].contains(&15) || high == low {
            return Err(Self::unsupported_arm(address, opcode));
        }

        let left = self.registers[multiplicand];
        let right = self.registers[multiplier];
        let mut product = if signed {
            (i64::from(left as i32) * i64::from(right as i32)) as u64
        } else {
            u64::from(left) * u64::from(right)
        };
        if accumulate {
            let addend = (u64::from(self.registers[high]) << 32) | u64::from(self.registers[low]);
            product = product.wrapping_add(addend);
        }
        if set_flags {
            self.cpsr = (self.cpsr & !(FLAG_N | FLAG_Z))
                | if product >> 63 != 0 { FLAG_N } else { 0 }
                | if product == 0 { FLAG_Z } else { 0 };
        }
        self.registers[low] = product as u32;
        self.registers[high] = (product >> 32) as u32;
        Ok(())
    }

    fn arm_swap(
        &mut self,
        memory: &mut MemoryMap,
        opcode: u32,
        address: u32,
    ) -> Result<(), CpuError> {
        let byte_transfer = opcode & (1 << 22) != 0;
        let base = ((opcode >> 16) & 0xF) as usize;
        let destination = ((opcode >> 12) & 0xF) as usize;
        let source = (opcode & 0xF) as usize;
        if [base, destination, source].contains(&15) {
            return Err(Self::unsupported_arm(address, opcode));
        }

        let transfer_address = self.registers[base];
        let value = self.registers[source];
        let loaded = if byte_transfer {
            let loaded = u32::from(memory.read_u8(transfer_address));
            memory.write_u8(transfer_address, value as u8);
            loaded
        } else {
            let loaded = load_word(memory, transfer_address);
            memory.write_u32(transfer_address & !3, value);
            loaded
        };
        self.registers[destination] = loaded;
        Ok(())
    }

    fn arm_mrs(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
        let destination = ((opcode >> 12) & 0xF) as usize;
        let value = if opcode & (1 << 22) != 0 {
            match self.current_spsr_index() {
                Some(index) => self.spsr_bank[index],
                None => return Err(Self::unsupported_arm(address, opcode)),
            }
        } else {
            self.cpsr
        };
        if destination == 15 {
            return Err(Self::unsupported_arm(address, opcode));
        }
        self.registers[destination] = value;
        Ok(())
    }

    fn arm_msr(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
        let writes_spsr = opcode & (1 << 22) != 0;
        let field_mask = (opcode >> 16) & 0xF;
        let value = if opcode & (1 << 25) != 0 {
            (opcode & 0xFF).rotate_right(((opcode >> 8) & 0xF) * 2)
        } else {
            self.read_register((opcode & 0xF) as usize, ExecutionState::Arm, address)
        };
        let mut write_mask = 0;
        for field in 0..4 {
            if field_mask & (1 << field) != 0 {
                write_mask |= 0xFF << (field * 8);
            }
        }

        if writes_spsr {
            let Some(index) = self.current_spsr_index() else {
                return Err(Self::unsupported_arm(address, opcode));
            };
            self.spsr_bank[index] = (self.spsr_bank[index] & !write_mask) | (value & write_mask);
            return Ok(());
        }

        // User mode may only change the condition flags.
        if self.cpsr & CPSR_MODE_MASK == CPSR_USER_MODE {
            write_mask &= 0xFF00_0000;
        }
        let updated_cpsr = (self.cpsr & !write_mask) | (value & write_mask);
        let updated_mode = updated_cpsr & CPSR_MODE_MASK;
        if !Self::is_valid_mode(updated_mode) || (updated_cpsr ^ self.cpsr) & CPSR_THUMB != 0 {
            return Err(Self::unsupported_arm(address, opcode));
        }
        self.switch_mode(updated_mode);
        self.cpsr = updated_cpsr;
        Ok(())
    }

    fn arm_single_transfer(
        &mut self,
        memory: &mut MemoryMap,
        opcode: u32,
        address: u32,
    ) -> Result<(), CpuError> {
        let register_offset = opcode & (1 << 25) != 0;
        if register_offset && opcode & (1 << 4) != 0 {
            return Err(Self::unsupported_arm(address, opcode));
        }
        let pre_index = opcode & (1 << 24) != 0;
        let add_offset = opcode & (1 << 23) != 0;
        let byte_transfer = opcode & (1 << 22) != 0;
        let writeback = opcode & (1 << 21) != 0;
        let load = opcode & (1 << 20) != 0;
        let base_register = ((opcode >> 16) & 0xF) as usize;
        let destination = ((opcode >> 12) & 0xF) as usize;
        let writes_back = !pre_index || writeback;
        if writes_back && base_register == 15 {
            return Err(Self::unsupported_arm(address, opcode));
        }

        let base = self.read_register(base_register, ExecutionState::Arm, address);
        let offset = if register_offset {
            let value = self.read_register((opcode & 0xF) as usize, ExecutionState::Arm, address);
            let shift_type = (opcode >> 5) & 0x3;
            let shift_amount = (opcode >> 7) & 0x1F;
            shift_immediate(value, shift_type, shift_amount, self.cpsr & FLAG_C != 0).0
        } else {
            opcode & 0xFFF
        };
        let adjusted_base = if add_offset {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let transfer_address = if pre_index { adjusted_base } else { base };

        if load {
            let value = if byte_transfer {
                u32::from(memory.read_u8(transfer_address))
            } else {
                load_word(memory, transfer_address)
            };
            // Base writeback happens first, so a load into the base register wins.
            if writes_back {
                self.registers[base_register] = adjusted_base;
            }
            self.write_register_or_pc(destination, value);
        } else {
            let value = if destination == 15 {
                address.wrapping_add(12)
            } else {
                self.registers[destination]
            };
            if byte_transfer {
                memory.write_u8(transfer_address, value as u8);
            } else {
                memory.write_u32(transfer_address & !3, value);
            }
            if writes_back {
                self.registers[base_register] = adjusted_base;
            }
        }
        Ok(())
    }

    fn arm_halfword_transfer(
        &mut self,
        memory: &mut MemoryMap,
        opcode: u32,
        address: u32,
    ) -> Result<(), CpuError> {
        let pre_index = opcode & (1 << 24) != 0;
        let add_offset = opcode & (1 << 23) != 0;
        let immediate_offset = opcode & (1 << 22) != 0;
        let writeback = opcode & (1 << 21) != 0;
        let load = opcode & (1 << 20) != 0;
        let base_register = ((opcode >> 16) & 0xF) as usize;
        let destination = ((opcode >> 12) & 0xF) as usize;
        let signed = opcode & (1 << 6) != 0;
        let halfword = opcode & (1 << 5) != 0;
        let writes_back = !pre_index || writeback;

        // Signed stores are ARMv5 doubleword transfers.
        if (!load && signed) || (writes_back && base_register == 15) {
            return Err(Self::unsupported_arm(address, opcode));
        }

        let base = self.read_register(base_register, ExecutionState::Arm, address);
        let offset = if immediate_offset {
            ((opcode >> 8) & 0xF) << 4 | (opcode & 0xF)
        } else {
            self.read_register((opcode & 0xF) as usize, ExecutionState::Arm, address)
        };
        let adjusted_base = if add_offset {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let transfer_address = if pre_index { adjusted_base } else { base };

        if load {
            let value = match (signed, halfword) {
                (false, true) => load_halfword(memory, transfer_address),
                (true, false) => load_signed_byte(memory, transfer_address),
                (true, true) => load_signed_halfword(memory, transfer_address),
                (false, false) => unreachable!(),
            };
            if writes_back {
                self.registers[base_register] = adjusted_base;
            }
            self.write_register_or_pc(destination, value);
        } else {
            let value = if destination == 15 {
                address.wrapping_add(12)
            } else {
                self.registers[destination]
            };
            memory.write_u16(transfer_address & !1, value as u16);
            if writes_back {
                self.registers[base_register] = adjusted_base;
            }
        }
        Ok(())
    }

    fn arm_block_transfer(
        &mut self,
        memory: &mut MemoryMap,
        opcode: u32,
        address: u32,
    ) -> Result<(), CpuError> {
        let pre_index = opcode & (1 << 24) != 0;
        let increment = opcode & (1 << 23) != 0;
        let psr_or_user = opcode & (1 << 22) != 0;
        let writeback = opcode & (1 << 21) != 0;
        let load = opcode & (1 << 20) != 0;
        let base_register = ((opcode >> 16) & 0xF) as usize;
        let encoded_list = opcode & 0xFFFF;
        if base_register == 15 {
            return Err(Self::unsupported_arm(address, opcode));
        }

        // ARMv4: an empty list transfers only PC but still moves the base by 16 words.
        let (register_list, register_count) = if encoded_list == 0 {
            (1 << 15, 16)
        } else {
            (encoded_list, encoded_list.count_ones())
        };
        let restores_cpsr = psr_or_user && load && register_list & (1 << 15) != 0;
        let user_bank_transfer = psr_or_user && !restores_cpsr;

        let base = self.registers[base_register];
        let span = register_count * 4;
        let mut transfer_address = match (increment, pre_index) {
            (true, false) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(span).wrapping_add(4),
            (false, true) => base.wrapping_sub(span),
        };
        let updated_base = if increment {
            base.wrapping_add(span)
        } else {
            base.wrapping_sub(span)
        };

        if load {
            // Writeback first: if the base is also loaded, the loaded value wins (ARMv4).
            if writeback {
                self.registers[base_register] = updated_base;
            }
            for register in 0..16 {
                if register_list & (1 << register) == 0 {
                    continue;
                }
                let value = memory.read_u32(transfer_address & !3);
                if register == 15 {
                    self.registers[15] = value;
                    if restores_cpsr {
                        self.restore_cpsr_from_spsr();
                    }
                    let alignment = if self.is_thumb() { !1 } else { !3 };
                    self.registers[15] &= alignment;
                } else if user_bank_transfer {
                    self.write_user_register(register, value);
                } else {
                    self.registers[register] = value;
                }
                transfer_address = transfer_address.wrapping_add(4);
            }
        } else {
            let first_register = register_list.trailing_zeros() as usize;
            for register in 0..16 {
                if register_list & (1 << register) == 0 {
                    continue;
                }
                // A stored base is the original value only when it is the first register.
                let value = if register == 15 {
                    address.wrapping_add(12)
                } else if register == base_register && writeback && register != first_register {
                    updated_base
                } else if user_bank_transfer {
                    self.read_user_register(register)
                } else {
                    self.registers[register]
                };
                memory.write_u32(transfer_address & !3, value);
                transfer_address = transfer_address.wrapping_add(4);
            }
            if writeback {
                self.registers[base_register] = updated_base;
            }
        }
        Ok(())
    }

    /// Writes a data-processing or load result, aligning PC for the current state.
    fn write_register_or_pc(&mut self, register: usize, value: u32) {
        if register == 15 {
            self.registers[15] = if self.is_thumb() {
                value & !1
            } else {
                value & !3
            };
        } else {
            self.registers[register] = value;
        }
    }

    fn branch_exchange(&mut self, target: u32) {
        if target & 1 != 0 {
            self.cpsr |= CPSR_THUMB;
            self.registers[15] = target & !1;
        } else {
            self.cpsr &= !CPSR_THUMB;
            self.registers[15] = target & !3;
        }
    }

    fn step_thumb(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
        let address = self.registers[15] & !1;
        let opcode = memory.read_u16(address);
        self.registers[15] = address.wrapping_add(2);

        if opcode & 0xF800 == 0x2000 {
            let destination = usize::from((opcode >> 8) & 0x7);
            let value = u32::from(opcode & 0x00FF);
            self.registers[destination] = value;
            self.set_nz(value);
            return Ok(());
        }

        if opcode & 0xF800 == 0x2800 {
            let source = self.registers[usize::from((opcode >> 8) & 0x7)];
            let immediate = u32::from(opcode & 0x00FF);
            self.set_sub_flags(source, immediate, source.wrapping_sub(immediate));
            return Ok(());
        }

        if matches!(opcode & 0xF800, 0x0000 | 0x0800 | 0x1000) {
            let shift_type = u32::from((opcode >> 11) & 0x3);
            let amount = u32::from((opcode >> 6) & 0x1F);
            let source = usize::from((opcode >> 3) & 0x7);
            let destination = usize::from(opcode & 0x7);
            let (result, carry) = shift_immediate(
                self.registers[source],
                shift_type,
                amount,
                self.cpsr & FLAG_C != 0,
            );
            self.registers[destination] = result;
            self.set_nz(result);
            self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
            return Ok(());
        }

        if opcode & 0xF800 == 0x1800 {
            let subtract = opcode & 0x0200 != 0;
            let immediate_operand = opcode & 0x0400 != 0;
            let operand = if immediate_operand {
                u32::from((opcode >> 6) & 0x7)
            } else {
                self.registers[usize::from((opcode >> 6) & 0x7)]
            };
            let source = self.registers[usize::from((opcode >> 3) & 0x7)];
            let destination = usize::from(opcode & 0x7);
            let result = if subtract {
                let value = source.wrapping_sub(operand);
                self.set_sub_flags(source, operand, value);
                value
            } else {
                let wide_result = u64::from(source) + u64::from(operand);
                let value = wide_result as u32;
                self.set_add_flags(source, operand, value, wide_result > u64::from(u32::MAX));
                value
            };
            self.registers[destination] = result;
            return Ok(());
        }

        if opcode & 0xF800 == 0x3000 || opcode & 0xF800 == 0x3800 {
            let destination = usize::from((opcode >> 8) & 0x7);
            let immediate = u32::from(opcode & 0x00FF);
            let left = self.registers[destination];
            let result = if opcode & 0xF800 == 0x3000 {
                let wide_result = u64::from(left) + u64::from(immediate);
                let result = wide_result as u32;
                self.set_add_flags(left, immediate, result, wide_result > u64::from(u32::MAX));
                result
            } else {
                let result = left.wrapping_sub(immediate);
                self.set_sub_flags(left, immediate, result);
                result
            };
            self.registers[destination] = result;
            return Ok(());
        }

        if opcode & 0xF800 == 0x4800 {
            let destination = usize::from((opcode >> 8) & 0x7);
            let literal_address = self.read_register(15, ExecutionState::Thumb, address) & !3;
            let literal_address = literal_address.wrapping_add(u32::from(opcode & 0x00FF) * 4);
            self.registers[destination] = memory.read_u32(literal_address);
            return Ok(());
        }

        if opcode & 0xF000 == 0xA000 {
            let destination = usize::from((opcode >> 8) & 0x7);
            let base = if opcode & 0x0800 != 0 {
                self.registers[13]
            } else {
                self.read_register(15, ExecutionState::Thumb, address) & !3
            };
            self.registers[destination] = base.wrapping_add(u32::from(opcode & 0x00FF) * 4);
            return Ok(());
        }

        if opcode & 0xF000 == 0x9000 {
            let load = opcode & 0x0800 != 0;
            let destination = usize::from((opcode >> 8) & 0x7);
            let offset = u32::from(opcode & 0x00FF) * 4;
            let transfer_address = self.registers[13].wrapping_add(offset);
            if load {
                self.registers[destination] = load_word(memory, transfer_address);
            } else {
                memory.write_u32(transfer_address & !3, self.registers[destination]);
            }
            return Ok(());
        }

        if opcode & 0xFE00 == 0xB400 {
            let registers = u32::from(opcode & 0x00FF);
            let include_lr = opcode & 0x0100 != 0;
            let register_count = registers.count_ones() + u32::from(include_lr);
            if register_count == 0 {
                return Err(CpuError::UnsupportedInstruction {
                    address,
                    state: ExecutionState::Thumb,
                    opcode,
                });
            }
            let new_stack = self.registers[13].wrapping_sub(register_count * 4);
            let mut address = new_stack;

            for register in 0..8 {
                if registers & (1 << register) != 0 {
                    memory.write_u32(address, self.registers[register]);
                    address = address.wrapping_add(4);
                }
            }
            if include_lr {
                memory.write_u32(address, self.registers[14]);
            }

            self.registers[13] = new_stack;
            return Ok(());
        }

        if opcode & 0xFE00 == 0xBC00 {
            let registers = u32::from(opcode & 0x00FF);
            let include_pc = opcode & 0x0100 != 0;
            let register_count = registers.count_ones() + u32::from(include_pc);
            if register_count == 0 {
                return Err(CpuError::UnsupportedInstruction {
                    address,
                    state: ExecutionState::Thumb,
                    opcode,
                });
            }
            let mut address = self.registers[13];

            for register in 0..8 {
                if registers & (1 << register) != 0 {
                    self.registers[register] = memory.read_u32(address);
                    address = address.wrapping_add(4);
                }
            }
            if include_pc {
                // ARMv4T ignores bit 0 here: POP {pc} never leaves Thumb state.
                self.registers[15] = memory.read_u32(address) & !1;
            }

            self.registers[13] = self.registers[13].wrapping_add(register_count * 4);
            return Ok(());
        }

        if opcode & 0xFF00 == 0xB000 {
            let offset = u32::from(opcode & 0x7F) * 4;
            if opcode & 0x0080 != 0 {
                self.registers[13] = self.registers[13].wrapping_sub(offset);
            } else {
                self.registers[13] = self.registers[13].wrapping_add(offset);
            }
            return Ok(());
        }

        if opcode & 0xF000 == 0x5000 {
            let offset = self.registers[usize::from((opcode >> 6) & 0x7)];
            let base = self.registers[usize::from((opcode >> 3) & 0x7)];
            let source_or_destination = usize::from(opcode & 0x7);
            let transfer_address = base.wrapping_add(offset);
            let value = self.registers[source_or_destination];

            match (opcode >> 9) & 0x7 {
                0 => memory.write_u32(transfer_address & !3, value),
                1 => memory.write_u16(transfer_address & !1, value as u16),
                2 => memory.write_u8(transfer_address, value as u8),
                operation => {
                    self.registers[source_or_destination] = match operation {
                        3 => load_signed_byte(memory, transfer_address),
                        4 => load_word(memory, transfer_address),
                        5 => load_halfword(memory, transfer_address),
                        6 => u32::from(memory.read_u8(transfer_address)),
                        7 => load_signed_halfword(memory, transfer_address),
                        _ => unreachable!(),
                    };
                }
            }
            return Ok(());
        }

        if matches!(opcode & 0xF000, 0x6000 | 0x7000) {
            let byte_transfer = opcode & 0x1000 != 0;
            if byte_transfer {
                let is_load = opcode & 0x0800 != 0;
                let base = usize::from((opcode >> 3) & 0x7);
                let source_or_destination = usize::from(opcode & 0x7);
                let offset = u32::from((opcode >> 6) & 0x1F);
                let transfer_address = self.registers[base].wrapping_add(offset);

                if is_load {
                    self.registers[source_or_destination] =
                        u32::from(memory.read_u8(transfer_address));
                } else {
                    memory.write_u8(
                        transfer_address,
                        self.registers[source_or_destination] as u8,
                    );
                }
                return Ok(());
            }

            let is_load = opcode & 0x0800 != 0;
            let base = usize::from((opcode >> 3) & 0x7);
            let source_or_destination = usize::from(opcode & 0x7);
            let offset = u32::from((opcode >> 6) & 0x1F) * 4;
            let address = self.registers[base].wrapping_add(offset);

            if is_load {
                self.registers[source_or_destination] = load_word(memory, address);
            } else {
                memory.write_u32(address & !3, self.registers[source_or_destination]);
            }
            return Ok(());
        }

        if opcode & 0xF000 == 0x8000 {
            let is_load = opcode & 0x0800 != 0;
            let offset = u32::from((opcode >> 6) & 0x1F) * 2;
            let base = usize::from((opcode >> 3) & 0x7);
            let source_or_destination = usize::from(opcode & 0x7);
            let transfer_address = self.registers[base].wrapping_add(offset);

            if is_load {
                self.registers[source_or_destination] = load_halfword(memory, transfer_address);
            } else {
                memory.write_u16(
                    transfer_address & !1,
                    self.registers[source_or_destination] as u16,
                );
            }
            return Ok(());
        }

        if opcode & 0xF000 == 0xC000 {
            let load = opcode & 0x0800 != 0;
            let base_register = usize::from((opcode >> 8) & 0x7);
            let encoded_list = u32::from(opcode & 0x00FF);
            let initial_address = self.registers[base_register];

            // ARMv4: an empty list transfers only PC but still moves the base by 16 words.
            if encoded_list == 0 {
                if load {
                    self.registers[15] = memory.read_u32(initial_address & !3) & !1;
                } else {
                    memory.write_u32(initial_address & !3, address.wrapping_add(6));
                }
                self.registers[base_register] = initial_address.wrapping_add(0x40);
                return Ok(());
            }

            let updated_base = initial_address.wrapping_add(encoded_list.count_ones() * 4);
            let first_register = encoded_list.trailing_zeros() as usize;
            let mut transfer_address = initial_address;
            for register in 0..8 {
                if encoded_list & (1 << register) == 0 {
                    continue;
                }
                if load {
                    self.registers[register] = memory.read_u32(transfer_address & !3);
                } else {
                    // A stored base is the original value only when it is the first register.
                    let value = if register == base_register && register != first_register {
                        updated_base
                    } else {
                        self.registers[register]
                    };
                    memory.write_u32(transfer_address & !3, value);
                }
                transfer_address = transfer_address.wrapping_add(4);
            }

            if !load || encoded_list & (1 << base_register) == 0 {
                self.registers[base_register] = updated_base;
            }
            return Ok(());
        }

        if opcode & 0xF000 == 0xD000 {
            let condition = ((opcode >> 8) & 0xF) as u8;
            if condition >= 0xE {
                if condition == 0xF {
                    let service = (opcode & 0x00FF) as u8;
                    self.execute_bios_call(memory, address, ExecutionState::Thumb, service)?;
                    return Ok(());
                }
                return Err(CpuError::UnsupportedInstruction {
                    address,
                    state: ExecutionState::Thumb,
                    opcode,
                });
            }
            if self.condition_passed(condition) {
                let offset = sign_extend(u32::from(opcode & 0x00FF), 8) << 1;
                self.registers[15] = address.wrapping_add(4).wrapping_add(offset);
            }
            return Ok(());
        }

        if opcode & 0xFC00 == 0x4400 {
            let operation = (opcode >> 8) & 0x3;
            let destination = usize::from((opcode & 0x7) | ((opcode >> 4) & 0x8));
            let source = usize::from((opcode >> 3) & 0xF);
            let left = self.read_register(destination, ExecutionState::Thumb, address);
            let right = self.read_register(source, ExecutionState::Thumb, address);

            match operation {
                0 => {
                    let result = left.wrapping_add(right);
                    if destination == 15 {
                        self.registers[15] = result & !1;
                    } else {
                        self.registers[destination] = result;
                    }
                }
                1 => self.set_sub_flags(left, right, left.wrapping_sub(right)),
                2 => {
                    if destination == 15 {
                        self.registers[15] = right & !1;
                    } else {
                        self.registers[destination] = right;
                    }
                }
                3 => {
                    if opcode & 0x0087 != 0 {
                        return Err(CpuError::UnsupportedInstruction {
                            address,
                            state: ExecutionState::Thumb,
                            opcode,
                        });
                    }
                    self.branch_exchange(right);
                }
                _ => unreachable!(),
            }
            return Ok(());
        }

        if opcode & 0xFC00 == 0x4000 {
            let operation = (opcode >> 6) & 0xF;
            let source_register = usize::from((opcode >> 3) & 0x7);
            let destination = usize::from(opcode & 0x7);
            let left = self.registers[destination];
            let right = self.registers[source_register];
            let carry_in = self.cpsr & FLAG_C != 0;
            let (result, write_result) = match operation {
                0 => (left & right, true),
                1 => (left ^ right, true),
                2 => {
                    let (value, carry) = shift_register(left, right, 0, carry_in);
                    self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
                    (value, true)
                }
                3 => {
                    let (value, carry) = shift_register(left, right, 1, carry_in);
                    self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
                    (value, true)
                }
                4 => {
                    let (value, carry) = shift_register(left, right, 2, carry_in);
                    self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
                    (value, true)
                }
                5 => {
                    let result = left.wrapping_add(right).wrapping_add(u32::from(carry_in));
                    self.set_arithmetic_flags(false, left, right, u32::from(carry_in), result);
                    (result, true)
                }
                6 => {
                    let borrow = u32::from(!carry_in);
                    let result = left.wrapping_sub(right).wrapping_sub(borrow);
                    self.set_arithmetic_flags(true, left, right, borrow, result);
                    (result, true)
                }
                7 => {
                    let (value, carry) = shift_register(left, right, 3, carry_in);
                    self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
                    (value, true)
                }
                8 => (left & right, false),
                9 => {
                    let result = 0u32.wrapping_sub(right);
                    self.set_sub_flags(0, right, result);
                    (result, true)
                }
                10 => {
                    let result = left.wrapping_sub(right);
                    self.set_sub_flags(left, right, result);
                    (result, false)
                }
                11 => {
                    let result = left.wrapping_add(right);
                    self.set_add_flags(
                        left,
                        right,
                        result,
                        u64::from(left) + u64::from(right) > u64::from(u32::MAX),
                    );
                    (result, false)
                }
                12 => (left | right, true),
                13 => (left.wrapping_mul(right), true),
                14 => (left & !right, true),
                15 => (!right, true),
                _ => unreachable!(),
            };

            if !matches!(operation, 5 | 6 | 9 | 10 | 11) {
                self.set_nz(result);
            }
            if write_result {
                self.registers[destination] = result;
            }
            return Ok(());
        }

        if opcode & 0xF800 == 0xF000 {
            let offset = sign_extend(u32::from(opcode & 0x07FF) << 12, 23);
            self.registers[14] = address.wrapping_add(4).wrapping_add(offset);
            return Ok(());
        }

        if opcode & 0xF800 == 0xF800 {
            let return_address = address.wrapping_add(2);
            let target = self.registers[14].wrapping_add(u32::from(opcode & 0x07FF) << 1);
            self.registers[14] = return_address | 1;
            self.registers[15] = target & !1;
            return Ok(());
        }

        if opcode & 0xF800 == 0xE000 {
            let offset = sign_extend(u32::from(opcode & 0x07FF), 11) << 1;
            self.registers[15] = address.wrapping_add(4).wrapping_add(offset);
            return Ok(());
        }

        Err(CpuError::UnsupportedInstruction {
            address,
            state: ExecutionState::Thumb,
            opcode,
        })
    }

    fn set_nz(&mut self, result: u32) {
        self.cpsr = (self.cpsr & !(FLAG_N | FLAG_Z))
            | if result & FLAG_N != 0 { FLAG_N } else { 0 }
            | if result == 0 { FLAG_Z } else { 0 };
    }

    fn set_add_flags(&mut self, left: u32, right: u32, result: u32, carry: bool) {
        self.set_nz(result);
        let overflow = (!(left ^ right) & (left ^ result) & FLAG_N) != 0;
        self.cpsr = (self.cpsr & !(FLAG_C | FLAG_V))
            | if carry { FLAG_C } else { 0 }
            | if overflow { FLAG_V } else { 0 };
    }

    fn set_sub_flags(&mut self, left: u32, right: u32, result: u32) {
        self.set_nz(result);
        let carry = left >= right;
        let overflow = ((left ^ right) & (left ^ result) & FLAG_N) != 0;
        self.cpsr = (self.cpsr & !(FLAG_C | FLAG_V))
            | if carry { FLAG_C } else { 0 }
            | if overflow { FLAG_V } else { 0 };
    }

    fn set_cv(&mut self, carry: bool, overflow: bool) {
        self.cpsr = (self.cpsr & !(FLAG_C | FLAG_V))
            | if carry { FLAG_C } else { 0 }
            | if overflow { FLAG_V } else { 0 };
    }

    fn set_arithmetic_flags(
        &mut self,
        subtract: bool,
        left: u32,
        right: u32,
        adjustment: u32,
        result: u32,
    ) {
        self.set_nz(result);
        let (carry, overflow) = if subtract {
            let subtrahend = u64::from(right) + u64::from(adjustment);
            let signed_result =
                i64::from(left as i32) - i64::from(right as i32) - i64::from(adjustment);
            (
                u64::from(left) >= subtrahend,
                signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
            )
        } else {
            let wide_result = u64::from(left) + u64::from(right) + u64::from(adjustment);
            let signed_result =
                i64::from(left as i32) + i64::from(right as i32) + i64::from(adjustment);
            (
                wide_result > u64::from(u32::MAX),
                signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
            )
        };
        self.set_cv(carry, overflow);
    }

    fn register_ram_reset(&self, memory: &mut MemoryMap, flags: u32) {
        if flags & (1 << 0) != 0 {
            memory.ewram.fill(0);
        }
        if flags & (1 << 1) != 0 {
            let clear_end = memory.iwram.len() - 0x200;
            memory.iwram[..clear_end].fill(0);
        }
        if flags & (1 << 2) != 0 {
            memory.palette.fill(0);
        }
        if flags & (1 << 3) != 0 {
            memory.vram.fill(0);
        }
        if flags & (1 << 4) != 0 {
            memory.oam.fill(0);
        }
        memory.reset_io_groups(
            flags & (1 << 5) != 0,
            flags & (1 << 6) != 0,
            flags & (1 << 7) != 0,
        );
        // The hardware BIOS always leaves the screen in forced blank, whatever the flags.
        // Games rely on it: Pokémon FireRed's boot only writes DISPSTAT (and so enables the
        // VCount IRQ that drives its sound DMA) directly while forced blank is on.
        memory.write_u16(crate::memory::DISPCNT_ADDR, 0x0080);
    }

    fn cpu_set(&self, memory: &mut MemoryMap) {
        let control = self.registers[2];
        let word_transfer = control & (1 << 26) != 0;
        let fixed_source = control & (1 << 24) != 0;
        let count = control & 0x001F_FFFF;
        let unit_size = if word_transfer { 4 } else { 2 };
        let mut source = self.registers[0] & !(unit_size - 1);
        let mut destination = self.registers[1] & !(unit_size - 1);

        for _ in 0..count {
            if word_transfer {
                let value = memory.read_u32(source);
                memory.write_u32(destination, value);
            } else {
                let value = memory.read_u16(source);
                memory.write_u16(destination, value);
            }
            if !fixed_source {
                source = source.wrapping_add(unit_size);
            }
            destination = destination.wrapping_add(unit_size);
        }
    }

    fn cpu_fast_set(&self, memory: &mut MemoryMap) {
        let control = self.registers[2];
        let fixed_source = control & (1 << 24) != 0;
        // The count is in words, rounded up to whole 8-word (32-byte) blocks.
        let word_count = ((control & 0x001F_FFFF) + 7) & !7;
        let mut source = self.registers[0] & !3;
        let mut destination = self.registers[1] & !3;

        for _ in 0..word_count {
            let value = memory.read_u32(source);
            memory.write_u32(destination, value);
            if !fixed_source {
                source = source.wrapping_add(4);
            }
            destination = destination.wrapping_add(4);
        }
    }

    /// `BgAffineSet`: r2 entries of {texture center x/y (19.8), screen center x/y, scale x/y
    /// (8.8), angle} at r0 become {PA, PB, PC, PD, reference X, reference Y} at r1.
    fn bg_affine_set(&self, memory: &mut MemoryMap) {
        let (mut source, mut destination) = (self.registers[0], self.registers[1]);
        for _ in 0..self.registers[2] {
            let half = |offset: u32| i32::from(memory.read_u16(source + offset) as i16);
            let (texture_x, texture_y) = (
                memory.read_u32(source) as i32,
                memory.read_u32(source + 4) as i32,
            );
            let (screen_x, screen_y) = (half(8), half(10));
            let [pa, pb, pc, pd] = affine_matrix(half(12), half(14), memory.read_u16(source + 16));
            for (index, parameter) in [pa, pb, pc, pd].into_iter().enumerate() {
                memory.write_u16(destination + index as u32 * 2, parameter as u16);
            }
            let reference_x = texture_x - (pa * screen_x + pb * screen_y);
            let reference_y = texture_y - (pc * screen_x + pd * screen_y);
            memory.write_u32(destination + 8, reference_x as u32);
            memory.write_u32(destination + 12, reference_y as u32);
            source = source.wrapping_add(20);
            destination = destination.wrapping_add(16);
        }
    }

    /// `ObjAffineSet`: r2 entries of {scale x/y (8.8), angle} at r0 become PA, PB, PC, PD at
    /// r1, each r3 bytes apart (2 for a packed array, 8 to write into OAM).
    fn obj_affine_set(&self, memory: &mut MemoryMap) {
        let (mut source, mut destination) = (self.registers[0], self.registers[1]);
        let stride = self.registers[3];
        for _ in 0..self.registers[2] {
            let scale_x = i32::from(memory.read_u16(source) as i16);
            let scale_y = i32::from(memory.read_u16(source + 2) as i16);
            let matrix = affine_matrix(scale_x, scale_y, memory.read_u16(source + 4));
            for parameter in matrix {
                memory.write_u16(destination, parameter as u16);
                destination = destination.wrapping_add(stride);
            }
            source = source.wrapping_add(8);
        }
    }

    fn execute_bios_call(
        &mut self,
        memory: &mut MemoryMap,
        address: u32,
        state: ExecutionState,
        service: u8,
    ) -> Result<(), CpuError> {
        // The hardware BIOS returns from every service through the same opcode.
        memory.set_bios_latch(BIOS_LATCH_AFTER_SWI);
        match service {
            0x01 if self.registers[0] & !0xFF == 0 => {
                self.register_ram_reset(memory, self.registers[0]);
                Ok(())
            }
            0x02 => {
                self.halt = Some(HaltState::Halt);
                Ok(())
            }
            0x04 | 0x05 => {
                let (discard_old_flags, mask) = if service == 0x05 {
                    (true, 1)
                } else {
                    (self.registers[0] != 0, self.registers[1] as u16 & 0x3FFF)
                };
                // The hardware BIOS never returns from a wait on an empty mask.
                if mask == 0 {
                    return Err(CpuError::UnsupportedBiosCall {
                        address,
                        state,
                        service,
                    });
                }

                memory.write_u8(IME_ADDR, 1);
                let resumed = std::mem::take(&mut self.intr_wait_resumed);
                let flags = memory.read_u16(BIOS_IRQ_FLAGS_ADDR);
                if discard_old_flags && !resumed {
                    memory.write_u16(BIOS_IRQ_FLAGS_ADDR, flags & !mask);
                } else if flags & mask != 0 {
                    memory.write_u16(BIOS_IRQ_FLAGS_ADDR, flags & !mask);
                    return Ok(());
                }
                self.halt = Some(HaltState::IntrWait {
                    swi_address: address,
                });
                Ok(())
            }
            0x06 | 0x07 => {
                let (numerator, denominator) = if service == 0x06 {
                    (self.registers[0] as i32, self.registers[1] as i32)
                } else {
                    (self.registers[1] as i32, self.registers[0] as i32)
                };
                // The hardware BIOS loops forever on division by zero.
                if denominator == 0 {
                    return Err(CpuError::UnsupportedBiosCall {
                        address,
                        state,
                        service,
                    });
                }
                let quotient = numerator.wrapping_div(denominator);
                self.registers[0] = quotient as u32;
                self.registers[1] = numerator.wrapping_rem(denominator) as u32;
                self.registers[3] = quotient.unsigned_abs();
                Ok(())
            }
            0x08 => {
                self.registers[0] = integer_square_root(self.registers[0]);
                Ok(())
            }
            0x09 => {
                let (angle, a, b) = arctan(self.registers[0] as i32);
                self.registers[0] = angle as u32;
                self.registers[1] = a as u32;
                self.registers[3] = b as u32;
                Ok(())
            }
            0x0A => {
                let x = self.registers[0] as i16 as i32;
                let y = self.registers[1] as i16 as i32;
                self.registers[0] = u32::from(arctan2(x, y));
                Ok(())
            }
            0x0B => {
                self.cpu_set(memory);
                Ok(())
            }
            0x0C => {
                self.cpu_fast_set(memory);
                Ok(())
            }
            0x0E => {
                self.bg_affine_set(memory);
                Ok(())
            }
            0x0F => {
                self.obj_affine_set(memory);
                Ok(())
            }
            0x11..=0x15 => {
                let source = self.registers[0];
                let (data, unit_size) = match service {
                    0x11 => (lz77_decompress(memory, source), 1),
                    0x12 => (lz77_decompress(memory, source), 2),
                    0x13 => (huffman_decompress(memory, source), 4),
                    0x14 => (run_length_decompress(memory, source), 1),
                    _ => (run_length_decompress(memory, source), 2),
                };
                let Some(data) = data else {
                    return Err(CpuError::UnsupportedBiosCall {
                        address,
                        state,
                        service,
                    });
                };
                write_decompressed(memory, self.registers[1], &data, unit_size);
                Ok(())
            }
            _ => Err(CpuError::UnsupportedBiosCall {
                address,
                state,
                service,
            }),
        }
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

    fn set_carry(&mut self, carry: bool) {
        self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
    }
}

/// Writes decompressed data in the unit size of the BIOS variant: bytes for the WRAM
/// variants, halfwords for VRAM, and words for Huffman. A trailing partial unit is dropped.
fn write_decompressed(memory: &mut MemoryMap, destination: u32, data: &[u8], unit_size: usize) {
    for (index, unit) in data.chunks_exact(unit_size).enumerate() {
        let address = destination.wrapping_add((index * unit_size) as u32);
        match unit {
            [byte] => memory.write_u8(address, *byte),
            [low, high] => memory.write_u16(address, u16::from_le_bytes([*low, *high])),
            _ => memory.write_u32(address, u32::from_le_bytes(unit.try_into().unwrap())),
        }
    }
}

/// Reads a BIOS compression header and returns the decompressed size. Like the hardware
/// BIOS, the type nibble is not checked; sources in the BIOS region (bits 25-27 clear)
/// make the call return without action, which is modeled as an empty output.
fn compression_header(memory: &MemoryMap, source: u32) -> Option<usize> {
    if source & 0x0E00_0000 == 0 {
        return Some(0);
    }
    Some((memory.read_u32(source & !3) >> 8) as usize)
}

/// Decodes a BIOS Huffman stream (header type 2) with 4- or 8-bit data units.
fn huffman_decompress(memory: &MemoryMap, source: u32) -> Option<Vec<u8>> {
    let size = compression_header(memory, source)?;
    if size == 0 {
        return Some(Vec::new());
    }
    let bits_per_unit = memory.read_u32(source & !3) & 0xF;
    if bits_per_unit != 4 && bits_per_unit != 8 {
        return None;
    }
    let tree = (source & !3).wrapping_add(4);
    let root = tree.wrapping_add(1);
    let mut stream = tree.wrapping_add((u32::from(memory.read_u8(tree)) + 1) * 2);
    let mut output = Vec::with_capacity(size + 3);
    let (mut word, mut filled_bits) = (0u32, 0);
    let mut node = root;

    while output.len() < size {
        let bits = memory.read_u32(stream);
        stream = stream.wrapping_add(4);
        for bit in (0..32).rev() {
            // Node: bits 0-5 child offset, bit 7/6 set when child 0/1 is a data leaf.
            let node_value = memory.read_u8(node);
            let direction = (bits >> bit) & 1;
            let child = (node & !1)
                .wrapping_add(u32::from(node_value & 0x3F) * 2 + 2)
                .wrapping_add(direction);
            if node_value & (0x80 >> direction) == 0 {
                node = child;
                continue;
            }
            let mask = (1 << bits_per_unit) - 1;
            word |= (u32::from(memory.read_u8(child)) & mask) << filled_bits;
            filled_bits += bits_per_unit;
            node = root;
            if filled_bits == 32 {
                output.extend_from_slice(&word.to_le_bytes());
                (word, filled_bits) = (0, 0);
                if output.len() >= size {
                    break;
                }
            }
        }
    }
    Some(output)
}

/// Decodes a BIOS run-length stream (header type 3).
fn run_length_decompress(memory: &MemoryMap, source: u32) -> Option<Vec<u8>> {
    let size = compression_header(memory, source)?;
    let mut output = Vec::with_capacity(size);
    let mut cursor = (source & !3).wrapping_add(4);
    while output.len() < size {
        let flag = memory.read_u8(cursor);
        cursor = cursor.wrapping_add(1);
        if flag & 0x80 != 0 {
            let byte = memory.read_u8(cursor);
            cursor = cursor.wrapping_add(1);
            output.extend(std::iter::repeat_n(byte, usize::from(flag & 0x7F) + 3));
        } else {
            for _ in 0..=flag {
                output.push(memory.read_u8(cursor));
                cursor = cursor.wrapping_add(1);
            }
        }
    }
    output.truncate(size);
    Some(output)
}

/// Decodes a BIOS LZ77 stream (header type 1).
fn lz77_decompress(memory: &MemoryMap, source: u32) -> Option<Vec<u8>> {
    let size = compression_header(memory, source)?;
    let mut output = Vec::with_capacity(size);
    let mut cursor = (source & !3).wrapping_add(4);
    let mut next_byte = || {
        let byte = memory.read_u8(cursor);
        cursor = cursor.wrapping_add(1);
        byte
    };

    while output.len() < size {
        let flags = next_byte();
        for block in 0..8 {
            if output.len() >= size {
                break;
            }
            if flags & (0x80 >> block) == 0 {
                output.push(next_byte());
                continue;
            }
            let first = next_byte();
            let second = next_byte();
            let length = usize::from(first >> 4) + 3;
            let distance = ((usize::from(first & 0xF) << 8) | usize::from(second)) + 1;
            for _ in 0..length {
                if output.len() >= size {
                    break;
                }
                // References before the start of the output read zero, as from cleared memory.
                let byte = output
                    .len()
                    .checked_sub(distance)
                    .map_or(0, |index| output[index]);
                output.push(byte);
            }
        }
    }
    Some(output)
}

/// Rotation/scale matrix [PA, PB, PC, PD] in 8.8 fixed point. Only the top byte of the
/// angle is used (256 steps per turn), with sine and cosine in 1.14 fixed point like the
/// BIOS table; individual values may differ from the hardware table by one unit.
fn affine_matrix(scale_x: i32, scale_y: i32, angle: u16) -> [i32; 4] {
    let theta = f64::from(angle >> 8) * std::f64::consts::TAU / 256.0;
    let sine = (theta.sin() * 16384.0).round() as i32;
    let cosine = (theta.cos() * 16384.0).round() as i32;
    [
        (scale_x * cosine) >> 14,
        (-scale_x * sine) >> 14,
        (scale_y * sine) >> 14,
        (scale_y * cosine) >> 14,
    ]
}

/// BIOS `ArcTan` polynomial for a 1.14 fixed-point tangent. Returns the angle
/// (0x4000 = 90 degrees) and the intermediate values the BIOS leaves in r1 and r3.
fn arctan(tangent: i32) -> (i32, i32, i32) {
    let a = -(tangent.wrapping_mul(tangent) >> 14);
    let mut b = ((0xA9 * a) >> 14) + 0x390;
    for coefficient in [0x91C, 0xFB6, 0x16AA, 0x2081, 0x3651, 0xA2F9] {
        b = (b.wrapping_mul(a) >> 14) + coefficient;
    }
    (tangent.wrapping_mul(b) >> 16, a, b)
}

/// BIOS `ArcTan2`: the angle of (x, y) as 0x0000-0xFFFF for a full turn.
fn arctan2(x: i32, y: i32) -> u16 {
    let arctan_of = |numerator: i32, denominator: i32| arctan((numerator << 14) / denominator).0;
    let angle = if y == 0 {
        if x >= 0 { 0 } else { 0x8000 }
    } else if x == 0 {
        if y >= 0 { 0x4000 } else { 0xC000 }
    } else if y >= 0 {
        if x >= 0 && x >= y {
            arctan_of(y, x)
        } else if x < 0 && -x >= y {
            arctan_of(y, x) + 0x8000
        } else {
            0x4000 - arctan_of(x, y)
        }
    } else if x <= 0 && -x > -y {
        arctan_of(y, x) + 0x8000
    } else if x > 0 && x >= -y {
        arctan_of(y, x) + 0x1_0000
    } else {
        0xC000 - arctan_of(x, y)
    };
    angle as u16
}

fn integer_square_root(value: u32) -> u32 {
    let mut root = (value as f64).sqrt() as u32;
    while u64::from(root) * u64::from(root) > u64::from(value) {
        root -= 1;
    }
    while u64::from(root + 1) * u64::from(root + 1) <= u64::from(value) {
        root += 1;
    }
    root
}

/// Internal multiply cycles (`m`) from the multiplier's significant bytes.
fn multiply_array_cycles(multiplier: u32, signed: bool) -> u32 {
    let significant = |mask: u32| {
        let high = multiplier & mask;
        high == 0 || (signed && high == mask)
    };
    if significant(0xFFFF_FF00) {
        1
    } else if significant(0xFFFF_0000) {
        2
    } else if significant(0xFF00_0000) {
        3
    } else {
        4
    }
}

/// Word load; a misaligned address rotates the aligned word right by the byte offset.
fn load_word(memory: &MemoryMap, address: u32) -> u32 {
    memory
        .read_u32(address & !3)
        .rotate_right((address & 3) * 8)
}

/// Halfword load; a misaligned address rotates the aligned halfword right by 8 (ARM7TDMI).
fn load_halfword(memory: &MemoryMap, address: u32) -> u32 {
    u32::from(memory.read_u16(address & !1)).rotate_right((address & 1) * 8)
}

fn load_signed_byte(memory: &MemoryMap, address: u32) -> u32 {
    i32::from(memory.read_u8(address) as i8) as u32
}

/// Signed halfword load; a misaligned address loads the sign-extended byte instead (ARM7TDMI).
fn load_signed_halfword(memory: &MemoryMap, address: u32) -> u32 {
    if address & 1 != 0 {
        load_signed_byte(memory, address)
    } else {
        i32::from(memory.read_u16(address) as i16) as u32
    }
}

fn sign_extend(value: u32, bits: u32) -> u32 {
    ((value << (32 - bits)) as i32 >> (32 - bits)) as u32
}

fn shift_immediate(value: u32, shift_type: u32, amount: u32, carry_in: bool) -> (u32, bool) {
    match shift_type {
        0 if amount == 0 => (value, carry_in),
        0 => (value << amount, value & (1 << (32 - amount)) != 0),
        1 if amount == 0 => (0, value & FLAG_N != 0),
        1 => (value >> amount, value & (1 << (amount - 1)) != 0),
        2 if amount == 0 => {
            let result = if value & FLAG_N != 0 { u32::MAX } else { 0 };
            (result, value & FLAG_N != 0)
        }
        2 => {
            let result = ((value as i32) >> amount) as u32;
            (result, value & (1 << (amount - 1)) != 0)
        }
        3 if amount == 0 => {
            let result = (u32::from(carry_in) << 31) | (value >> 1);
            (result, value & 1 != 0)
        }
        3 => {
            let result = value.rotate_right(amount);
            (result, result & FLAG_N != 0)
        }
        _ => unreachable!(),
    }
}

fn shift_register(
    value: u32,
    amount_register: u32,
    shift_type: u32,
    carry_in: bool,
) -> (u32, bool) {
    let amount = amount_register & 0xFF;
    if amount == 0 {
        return (value, carry_in);
    }

    match shift_type {
        0 if amount < 32 => (value << amount, value & (1 << (32 - amount)) != 0),
        0 if amount == 32 => (0, value & 1 != 0),
        0 => (0, false),
        1 if amount < 32 => (value >> amount, value & (1 << (amount - 1)) != 0),
        1 if amount == 32 => (0, value & FLAG_N != 0),
        1 => (0, false),
        2 if amount < 32 => {
            let result = ((value as i32) >> amount) as u32;
            (result, value & (1 << (amount - 1)) != 0)
        }
        2 => {
            let negative = value & FLAG_N != 0;
            (if negative { u32::MAX } else { 0 }, negative)
        }
        3 => {
            let rotation = amount & 0x1F;
            if rotation == 0 {
                (value, value & FLAG_N != 0)
            } else {
                let result = value.rotate_right(rotation);
                (result, result & FLAG_N != 0)
            }
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn arm_word(opcode: u32) -> Vec<u8> {
        opcode.to_le_bytes().to_vec()
    }

    #[test]
    fn arm_add_immediate_uses_architectural_pc_value() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE28F_0018)); // ADD r0, pc, #0x18
        let mut cpu = ThumbCpu::boot_hle();

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], ROM_BASE + 8 + 0x18);
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn arm_mov_register_reads_pc_as_instruction_address_plus_eight() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE1A0_E00F)); // MOV lr, pc
        let mut cpu = ThumbCpu::boot_hle();

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[14], ROM_BASE + 8);
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn arm_mrs_reads_cpsr_without_changing_flags() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE10F_2000)); // MRS r2, CPSR
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= FLAG_N | FLAG_C;
        let expected_cpsr = cpu.cpsr;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[2], expected_cpsr);
        assert_eq!(cpu.cpsr, expected_cpsr);
    }

    #[test]
    fn arm_mov_lsl_immediate_updates_shifter_carry() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE1B0_0081)); // MOVS r0, r1, LSL #1
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[1] = 0x8000_0001;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 2);
        assert_ne!(cpu.cpsr & FLAG_C, 0);
    }

    #[test]
    fn arm_mov_register_shift_uses_the_low_byte_of_the_shift_register() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE1B0_0211)); // MOVS r0, r1, LSL r2
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[1] = 0x8000_0001;
        cpu.registers[2] = 33;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0);
        assert_ne!(cpu.cpsr & FLAG_Z, 0);
        assert_eq!(cpu.cpsr & FLAG_C, 0);
    }

    #[test]
    fn arm_register_offset_ldr_applies_the_encoded_shift() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE791_0102)); // LDR r0, [r1, r2, LSL #2]
        memory.write_u32(0x0200_0010, 0x1234_5678);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[1] = 0x0200_0000;
        cpu.registers[2] = 4;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0x1234_5678);
    }

    #[test]
    fn arm_halfword_and_signed_transfers_use_their_correct_widths() {
        let mut memory = MemoryMap::new();
        let mut rom = arm_word(0xE1C0_10B2); // STRH r1, [r0, #2]
        rom.extend(arm_word(0xE1D0_20F2)); // LDRSH r2, [r0, #2]
        rom.extend(arm_word(0xE190_30D1)); // LDRSB r3, [r0, r1]
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0000_80FF;

        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u16(0x0200_0002), 0x80FF);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[2], 0xFFFF_80FF);

        memory.write_u8(0x0200_0004, 0x80);
        cpu.registers[1] = 4;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[3], 0xFFFF_FF80);
    }

    #[test]
    fn arm_swi_register_ram_reset_uses_the_same_hle_service_as_thumb() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xEF01_0000)); // SWI 0x10000 (RegisterRamReset)
        memory.ewram.fill(0xA5);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = 1;

        cpu.step(&mut memory).unwrap();

        assert!(memory.ewram.iter().all(|byte| *byte == 0));
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn arm_stmdb_and_ldmia_transfer_registers_in_ascending_order() {
        let mut memory = MemoryMap::new();
        let mut rom = arm_word(0xE92D_4010); // STMDB sp!, {r4, lr}
        rom.extend(arm_word(0xE8BD_0006)); // LDMIA sp!, {r1, r2}
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[13] = 0x0200_0100;
        cpu.registers[4] = 0x1234_5678;
        cpu.registers[14] = 0x9ABC_DEF0;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u32(0x0200_00F8), 0x1234_5678);
        assert_eq!(memory.read_u32(0x0200_00FC), 0x9ABC_DEF0);
        assert_eq!(cpu.registers[13], 0x0200_00F8);

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[1], 0x1234_5678);
        assert_eq!(cpu.registers[2], 0x9ABC_DEF0);
        assert_eq!(cpu.registers[13], 0x0200_0100);
    }

    #[test]
    fn arm_ldmia_can_restore_pc_and_write_back_the_stack() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE8BD_8010)); // LDMIA sp!, {r4, pc}
        memory.write_u32(0x0200_0100, 0x1234_5678);
        memory.write_u32(0x0200_0104, ROM_BASE + 0x40);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[13] = 0x0200_0100;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[4], 0x1234_5678);
        assert_eq!(cpu.registers[13], 0x0200_0108);
        assert_eq!(cpu.registers[15], ROM_BASE + 0x40);
        assert!(!cpu.is_thumb());
    }

    #[test]
    fn thumb_push_and_pop_register_list_round_trip_in_ascending_order() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xB5F0, 0xBDF0])); // PUSH {r4-r7, lr}; POP {r4-r7, pc}
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[13] = 0x0300_0100;
        cpu.registers[4] = 0x44;
        cpu.registers[5] = 0x55;
        cpu.registers[6] = 0x66;
        cpu.registers[7] = 0x77;
        cpu.registers[14] = ROM_BASE + 1;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[13], 0x0300_00EC);
        assert_eq!(memory.read_u32(0x0300_00EC), 0x44);
        assert_eq!(memory.read_u32(0x0300_00F0), 0x55);
        assert_eq!(memory.read_u32(0x0300_00F4), 0x66);
        assert_eq!(memory.read_u32(0x0300_00F8), 0x77);
        assert_eq!(memory.read_u32(0x0300_00FC), ROM_BASE + 1);

        cpu.registers[4..8].fill(0);
        cpu.step(&mut memory).unwrap();

        assert_eq!(&cpu.registers[4..8], &[0x44, 0x55, 0x66, 0x77]);
        assert_eq!(cpu.registers[13], 0x0300_0100);
        assert_eq!(cpu.registers[15], ROM_BASE);
        assert!(cpu.is_thumb());
    }

    #[test]
    fn thumb_add_sub_sp_adjusts_stack_by_scaled_immediate() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xB081, 0xB001])); // SUB sp, #4; ADD sp, #4
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[13] = 0x0300_0100;

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[13], 0x0300_00FC);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[13], 0x0300_0100);
    }

    #[test]
    fn thumb_add_pc_or_sp_to_register_uses_word_aligned_pc_and_scaled_offset() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x46C0, 0xA203, 0xAA02])); // NOP; ADD r2, PC, #12; ADD r2, SP, #8
        let mut cpu = ThumbCpu::new(ROM_BASE + 2);
        cpu.registers[13] = 0x0300_0100;

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[2], ROM_BASE + 4 + 12);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[2], 0x0300_0108);
    }

    #[test]
    fn thumb_sp_relative_word_store_and_load_use_scaled_offset() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x9301, 0x9B01])); // STR r3, [sp, #4]; LDR r3, [sp, #4]
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[13] = 0x0300_0100;
        cpu.registers[3] = 0x1234_5678;

        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u32(0x0300_0104), 0x1234_5678);

        cpu.registers[3] = 0;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[3], 0x1234_5678);
    }

    #[test]
    fn thumb_immediate_lsl_updates_result_and_carry() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x0492])); // LSLS r2, r2, #18
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[2] = 0x0000_4000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[2], 0x0000_0000);
        assert_ne!(cpu.cpsr & FLAG_Z, 0);
        assert_ne!(cpu.cpsr & FLAG_C, 0);
    }

    #[test]
    fn thumb_high_register_mov_copies_r8_to_r7() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x4647])); // MOV r7, r8
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[8] = 0x1234_5678;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[7], 0x1234_5678);
        assert_eq!(cpu.registers[15], ROM_BASE + 2);
    }

    #[test]
    fn thumb_tst_sets_nz_without_writing_the_register() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x4208])); // TST r0, r1
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0xF0;
        cpu.registers[1] = 0x0F;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0xF0);
        assert_ne!(cpu.cpsr & FLAG_Z, 0);
    }

    #[test]
    fn thumb_pop_pc_stays_in_thumb_state_on_armv4() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xBD00])); // POP {pc}
        memory.write_u32(0x0300_0100, ROM_BASE + 0x40);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[13] = 0x0300_0100;

        cpu.step(&mut memory).unwrap();

        assert!(cpu.is_thumb());
        assert_eq!(cpu.registers[15], ROM_BASE + 0x40);
    }

    #[test]
    fn thumb_high_register_mov_reads_unaligned_pc_plus_four() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x46C0, 0x4678])); // NOP; MOV r0, pc
        let mut cpu = ThumbCpu::new(ROM_BASE + 2);

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], ROM_BASE + 6);
    }

    #[test]
    fn thumb_register_offset_loads_apply_misaligned_quirks() {
        let mut memory = MemoryMap::new();
        // LDR r2, [r0, r1]; LDRH r3, [r0, r1]; LDRSH r4, [r0, r1]
        memory.load_cartridge(&program(&[0x5842, 0x5A43, 0x5E44]));
        memory.write_u32(0x0200_0000, 0x8877_6655);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 1;

        cpu.run_steps(&mut memory, 3).unwrap();

        assert_eq!(
            cpu.registers[2], 0x5588_7766,
            "word load rotates by the byte offset"
        );
        assert_eq!(cpu.registers[3], 0x5500_0066, "halfword load rotates by 8");
        assert_eq!(
            cpu.registers[4], 0x0000_0066,
            "misaligned LDRSH loads a signed byte"
        );
    }

    #[test]
    fn thumb_high_register_bx_switches_to_arm_state() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x4700])); // BX r0
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = ROM_BASE + 0x22;

        cpu.step(&mut memory).unwrap();

        assert!(!cpu.is_thumb());
        assert_eq!(cpu.registers[15], ROM_BASE + 0x20);
    }

    #[test]
    fn thumb_bl_prefix_and_suffix_branch_and_save_return_address() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xF000, 0xF802]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[14], ROM_BASE + 4);
        assert_eq!(cpu.registers[15], ROM_BASE + 2);

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[15], ROM_BASE + 8);
        assert_eq!(cpu.registers[14], ROM_BASE + 5);
        assert!(cpu.is_thumb());
    }

    #[test]
    fn thumb_bl_prefix_sign_extends_the_high_offset() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xF7FF, 0xF800]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        cpu.step(&mut memory).unwrap();

        assert_eq!(
            cpu.registers[14],
            ROM_BASE.wrapping_add(4).wrapping_sub(0x1000)
        );
    }

    #[test]
    fn thumb_swi_register_ram_reset_clears_selected_regions() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF01])); // SWI RegisterRamReset
        memory.ewram.fill(0xA5);
        memory.iwram.fill(0x5A);
        memory.palette.fill(0x11);
        memory.vram.fill(0x22);
        memory.oam.fill(0x33);
        memory.io.fill(0x44);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0b1110_1111;

        cpu.step(&mut memory).unwrap();

        assert!(memory.ewram.iter().all(|byte| *byte == 0));
        assert_eq!(memory.iwram[0], 0);
        assert_eq!(memory.iwram[memory.iwram.len() - 0x200], 0x5A);
        assert!(memory.palette.iter().all(|byte| *byte == 0));
        assert!(memory.vram.iter().all(|byte| *byte == 0));
        assert_eq!(memory.oam[0], 0x33);
        let mut expected_io = vec![0; memory.io.len()];
        for offset in [0x021, 0x027, 0x031, 0x037] {
            expected_io[offset] = 0x01; // BG2/BG3 affine PA and PD = 1.0
        }
        expected_io[0x089] = 0x02; // SOUNDBIAS = 0x200
        expected_io[0x000] = 0x80; // DISPCNT forced blank
        assert_eq!(memory.io, expected_io);
    }

    #[test]
    fn thumb_swi_cpuset_copies_word_ranges() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF0B])); // SWI CpuSet
        memory.write_u32(0x0200_0000, 0x1234_5678);
        memory.write_u32(0x0200_0004, 0x9ABC_DEF0);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0200_0100;
        cpu.registers[2] = (1 << 26) | 2;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u32(0x0200_0100), 0x1234_5678);
        assert_eq!(memory.read_u32(0x0200_0104), 0x9ABC_DEF0);
        assert_eq!(cpu.registers[15], ROM_BASE + 2);
    }

    #[test]
    fn thumb_swi_cpuset_fills_from_a_fixed_halfword_source() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF0B]));
        memory.write_u16(0x0200_0000, 0x55AA);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0200_0100;
        cpu.registers[2] = (1 << 24) | 3;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u16(0x0200_0100), 0x55AA);
        assert_eq!(memory.read_u16(0x0200_0102), 0x55AA);
        assert_eq!(memory.read_u16(0x0200_0104), 0x55AA);
    }

    #[test]
    fn thumb_swi_cpufastset_copies_eight_word_blocks() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF0C])); // SWI CpuFastSet
        for word in 0..8 {
            memory.write_u32(0x0200_0000 + word * 4, 0x1000_0000 + word);
        }
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0200_0100;
        cpu.registers[2] = 1;

        cpu.step(&mut memory).unwrap();

        for word in 0..8 {
            assert_eq!(memory.read_u32(0x0200_0100 + word * 4), 0x1000_0000 + word);
        }
    }

    /// ARM IRQ handler installed at 0x03007FFC: acknowledges VBlank in `IF` and reports it
    /// to the BIOS through the interrupt flags at 0x03007FF8, as games do.
    const VBLANK_HANDLER: [u32; 8] = [
        0xE3A0_3301, // MOV r3, #0x04000000
        0xE283_3C02, // ADD r3, r3, #0x200
        0xE3A0_2001, // MOV r2, #1
        0xE1C3_20B2, // STRH r2, [r3, #2]     (IF: acknowledge VBlank)
        0xE3A0_3403, // MOV r3, #0x03000000
        0xE283_3C7F, // ADD r3, r3, #0x7F00
        0xE1C3_2FB8, // STRH r2, [r3, #0xF8]  (BIOS interrupt flags)
        0xE12F_FF1E, // BX lr
    ];

    fn install_vblank_handler(memory: &mut MemoryMap, rom: &mut Vec<u8>, offset: usize) {
        rom.resize(offset, 0);
        for word in VBLANK_HANDLER {
            rom.extend(word.to_le_bytes());
        }
        memory.write_u32(0x0300_7FFC, ROM_BASE + offset as u32);
    }

    #[test]
    fn cpufastset_word_count_rounds_up_to_whole_blocks_without_overrunning() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF0C])); // SWI CpuFastSet
        memory.write_u32(0x0200_0000, 0x5555_AAAA);
        memory.write_u32(0x0300_0040, 0x1234_5678);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0300_0000;
        cpu.registers[2] = (1 << 24) | 9; // fill 9 words -> 16 words

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u32(0x0300_003C), 0x5555_AAAA);
        assert_eq!(memory.read_u32(0x0300_0040), 0x1234_5678);
    }

    /// "ABCABCABCX" as header + flags + literal A/B/C + back-reference(len 6, distance 3) + X.
    const LZ77_SAMPLE: [u8; 13] = [
        0x10, 0x0A, 0x00, 0x00, // type 1, size 10
        0x10, // block 3 is a back-reference
        b'A', b'B', b'C', 0x30, 0x02, // len 3 + 3, distance 2 + 1
        b'X', 0x00, 0x00,
    ];

    #[test]
    fn lz77_wram_and_vram_services_expand_back_references() {
        let mut rom = program(&[0xDF11, 0xDF12]); // LZ77UnCompWram; LZ77UnCompVram
        rom.resize(0x20, 0);
        rom.extend(LZ77_SAMPLE);
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = ROM_BASE + 0x20;
        cpu.registers[1] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();
        let expanded: Vec<u8> = (0..10).map(|i| memory.read_u8(0x0200_0000 + i)).collect();
        assert_eq!(&expanded, b"ABCABCABCX");

        cpu.registers[1] = 0x0600_0000;
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u16(0x0600_0000), u16::from_le_bytes(*b"AB"));
        assert_eq!(memory.read_u16(0x0600_0008), u16::from_le_bytes(*b"CX"));
    }

    #[test]
    fn lz77_service_ignores_the_header_type_like_the_hardware_bios() {
        let mut rom = program(&[0xDF11]);
        rom.resize(0x20, 0);
        let mut stream = LZ77_SAMPLE;
        stream[0] = 0x00; // type nibble 0 instead of 1
        rom.extend(stream);
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = ROM_BASE + 0x20;
        cpu.registers[1] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();
        let expanded: Vec<u8> = (0..10).map(|i| memory.read_u8(0x0200_0000 + i)).collect();
        assert_eq!(&expanded, b"ABCABCABCX");
    }

    #[test]
    fn bios_reads_after_a_service_return_the_swi_exit_opcode() {
        // MOVS r0, #0; LDR r1, [r0]; MOVS r1, #1; SWI 0x06 (Div); MOVS r0, #0; LDR r1, [r0]
        let rom = program(&[0x2000, 0x6801, 0x2101, 0xDF06, 0x2000, 0x6801]);
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::new(ROM_BASE);

        cpu.run_steps(&mut memory, 2).unwrap();
        assert_eq!(cpu.registers[1], 0xE129_F000);
        cpu.run_steps(&mut memory, 4).unwrap();
        assert_eq!(cpu.registers[1], BIOS_LATCH_AFTER_SWI);
    }

    #[test]
    fn decompression_services_skip_sources_in_the_bios_region() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF11, 0xDF13, 0xDF14]));
        memory.write_u32(0x0200_0000, 0xAAAA_AAAA);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        for _ in 0..3 {
            cpu.registers[0] = 0x0000_0100;
            cpu.registers[1] = 0x0200_0000;
            cpu.step(&mut memory).unwrap();
        }
        assert_eq!(memory.read_u32(0x0200_0000), 0xAAAA_AAAA);
    }

    #[test]
    fn obj_affine_set_writes_scaled_rotation_matrices_with_a_stride() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF0F]));
        // Entry 0: scale 1.0, angle 0. Entry 1: scale 2.0 x / 1.0 y, angle 90 degrees.
        for (offset, value) in [
            (0, 0x100),
            (2, 0x100),
            (4, 0),
            (8, 0x200),
            (10, 0x100),
            (12, 0x4000),
        ] {
            memory.write_u16(0x0200_0000 + offset, value);
        }
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0300_0000;
        cpu.registers[2] = 2;
        cpu.registers[3] = 8; // OAM layout

        cpu.step(&mut memory).unwrap();

        let parameter = |index: u32| memory.read_u16(0x0300_0000 + index * 8) as i16;
        assert_eq!([0, 1, 2, 3].map(parameter), [0x100, 0, 0, 0x100]);
        assert_eq!([4, 5, 6, 7].map(parameter), [0, -0x200, 0x100, 0]);
    }

    #[test]
    fn bg_affine_set_derives_the_reference_point_from_both_centers() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF0E]));
        memory.write_u32(0x0200_0000, 64 << 8); // texture center x
        memory.write_u32(0x0200_0004, 32 << 8); // texture center y
        memory.write_u16(0x0200_0008, 120); // screen center x
        memory.write_u16(0x0200_000A, 80); // screen center y
        memory.write_u16(0x0200_000C, 0x100); // scale x
        memory.write_u16(0x0200_000E, 0x100); // scale y
        memory.write_u16(0x0200_0010, 0); // angle
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 0x0300_0000;
        cpu.registers[2] = 1;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u16(0x0300_0000), 0x100);
        assert_eq!(memory.read_u16(0x0300_0006), 0x100);
        assert_eq!(memory.read_u32(0x0300_0008) as i32, (64 - 120) << 8);
        assert_eq!(memory.read_u32(0x0300_000C) as i32, (32 - 80) << 8);
    }

    #[test]
    fn arctan2_returns_full_turn_angles_for_each_quadrant() {
        let one = 0x4000; // 1.0 in 1.14 fixed point
        assert_eq!(arctan2(one, 0), 0);
        assert_eq!(arctan2(0, one), 0x4000);
        assert_eq!(arctan2(-one, 0), 0x8000);
        assert_eq!(arctan2(0, -one), 0xC000);
        // 45-degree diagonals land within a unit or two of the exact angle.
        for (x, y, expected) in [
            (one, one, 0x2000),
            (-one, one, 0x6000),
            (-one, -one, 0xA000),
            (one, -one, 0xE000),
        ] {
            let angle = i32::from(arctan2(x, y));
            assert!((angle - expected).abs() <= 2, "({x}, {y}) -> {angle:#x}");
        }
    }

    #[test]
    fn huffman_service_decodes_8_bit_units_into_words() {
        // Tree: root with two leaves, 'A' for bit 0 and 'B' for bit 1.
        let mut stream = vec![0x28, 0x04, 0x00, 0x00]; // type 2, 8-bit units, size 4
        stream.extend([0x01, 0xC0, b'A', b'B']); // tree size byte, root node, leaves
        stream.extend(0x6000_0000u32.to_le_bytes()); // bits 0,1,1,0 -> "ABBA"
        let mut rom = program(&[0xDF13]);
        rom.resize(0x20, 0);
        rom.extend(stream);
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = ROM_BASE + 0x20;
        cpu.registers[1] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u32(0x0200_0000), u32::from_le_bytes(*b"ABBA"));
    }

    #[test]
    fn run_length_service_expands_runs_and_literals() {
        let stream = [
            0x30, 0x07, 0x00, 0x00, // type 3, size 7
            0x81, b'Z', // run of 1 + 3 = 4 'Z'
            0x02, b'a', b'b', b'c', // 2 + 1 = 3 literals
        ];
        let mut rom = program(&[0xDF14]);
        rom.resize(0x20, 0);
        rom.extend(stream);
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = ROM_BASE + 0x20;
        cpu.registers[1] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        let expanded: Vec<u8> = (0..7).map(|i| memory.read_u8(0x0200_0000 + i)).collect();
        assert_eq!(&expanded, b"ZZZZabc");
    }

    #[test]
    fn thumb_vblank_intrwait_runs_the_game_irq_handler_and_returns_after_vblank() {
        let mut rom = program(&[0xDF05, 0x2001]); // VBlankIntrWait; MOVS r0, #1
        let mut memory = MemoryMap::new();
        install_vblank_handler(&mut memory, &mut rom, 0x40);
        memory.load_cartridge(&rom);
        memory.write_u8(crate::memory::DISPSTAT_ADDR, 1 << 3);
        memory.write_u16(crate::memory::IE_ADDR, 1);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= CPSR_THUMB;
        cpu.registers[0] = 0xFF;

        let mut steps = 0;
        while cpu.registers[0] != 1 {
            cpu.step(&mut memory).unwrap();
            steps += 1;
            assert!(steps < 400_000, "VBlankIntrWait never returned");
        }

        assert_eq!(cpu.registers[15], ROM_BASE + 4);
        assert!(cpu.is_thumb());
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SYSTEM_MODE);
        assert_eq!(cpu.registers[13], HLE_SYSTEM_STACK);
        assert_eq!(memory.read_u16(crate::memory::IF_ADDR), 0);
        assert_eq!(memory.read_u16(BIOS_IRQ_FLAGS_ADDR), 0);
    }

    #[test]
    fn intrwait_without_discard_returns_when_bios_flags_are_already_set() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF04, 0x2002])); // IntrWait; MOVS r0, #2
        memory.write_u16(BIOS_IRQ_FLAGS_ADDR, (1 << 3) | 1);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0;
        cpu.registers[1] = 1 << 3;

        cpu.step(&mut memory).unwrap();
        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 2);
        assert_eq!(memory.read_u16(BIOS_IRQ_FLAGS_ADDR), 1);
        assert_eq!(memory.read_u8(IME_ADDR), 1);
    }

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
    fn unsupported_thumb_swi_is_reported_explicitly() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF1F])); // MidiKey2Freq
        let mut cpu = ThumbCpu::new(ROM_BASE);

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedBiosCall {
                address: ROM_BASE,
                state: ExecutionState::Thumb,
                service: 0x1F,
            })
        );
        assert_eq!(cpu.registers[15], ROM_BASE);
    }

    #[test]
    fn register_ram_reset_clears_only_the_selected_register_groups() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF01, 0xDF01]));
        memory.write_u16(crate::memory::DISPCNT_ADDR, 0x1900);
        memory.write_u8(0x0400_0084, 0x80); // sound master enable
        memory.write_u16(0x0400_0080, 0x1234); // SOUNDCNT_L
        memory.write_u16(0x0400_0128, 0x5678); // SIOCNT
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 1 << 5; // serial only

        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.io[0x128..0x12A], [0, 0]);
        assert_eq!(
            memory.read_u16(crate::memory::DISPCNT_ADDR),
            0x0080,
            "every reset forces blank"
        );
        assert_eq!(memory.io[0x080..0x082], [0x34, 0x12]);

        memory.write_u16(crate::memory::DISPCNT_ADDR, 0x1900);
        cpu.registers[0] = 1 << 7; // other registers
        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u16(crate::memory::DISPCNT_ADDR), 0x0080);
        assert_eq!(
            memory.io[0x080..0x082],
            [0x34, 0x12],
            "sound is a separate group"
        );
    }

    #[test]
    fn register_ram_reset_rejects_unmodeled_flag_groups_without_clearing() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF01]));
        memory.ewram.fill(0xA5);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = (1 << 0) | (1 << 8);

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedBiosCall { service: 1, .. })
        ));
        assert!(memory.ewram.iter().all(|byte| *byte == 0xA5));
        assert_eq!(cpu.registers[15], ROM_BASE);
    }

    #[test]
    fn arm_adc_immediate_updates_nzcv_with_carry_in() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE2B0_0001)); // ADCS r0, r0, #1
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = 0x7FFF_FFFF;
        cpu.cpsr |= FLAG_C;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0x8000_0001);
        assert_ne!(cpu.cpsr & FLAG_N, 0);
        assert_eq!(cpu.cpsr & FLAG_Z, 0);
        assert_eq!(cpu.cpsr & FLAG_C, 0);
        assert_ne!(cpu.cpsr & FLAG_V, 0);
    }

    #[test]
    fn arm_rotated_immediate_sets_shifter_carry_and_preserves_overflow() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE3B0_0480)); // MOVS r0, #0x80000000
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= FLAG_V;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0x8000_0000);
        assert_ne!(cpu.cpsr & FLAG_N, 0);
        assert_ne!(cpu.cpsr & FLAG_C, 0);
        assert_ne!(cpu.cpsr & FLAG_V, 0);
    }

    #[test]
    fn arm_pc_relative_ldr_sp_handles_the_reported_opcode() {
        let mut rom = vec![0; 0x23C];
        rom[0x208..0x20C].copy_from_slice(&0xE59F_D028u32.to_le_bytes());
        rom[0x238..0x23C].copy_from_slice(&0x0300_7F00u32.to_le_bytes());
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[15] = ROM_BASE + 0x208;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[13], 0x0300_7F00);
        assert_eq!(cpu.registers[15], ROM_BASE + 0x20C);
    }

    #[test]
    fn arm_immediate_word_store_supports_preindex_and_writeback() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE5A2_1004)); // STR r1, [r2, #4]!
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[1] = 0x1234_5678;
        cpu.registers[2] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u32(0x0200_0004), 0x1234_5678);
        assert_eq!(cpu.registers[2], 0x0200_0004);
    }

    #[test]
    fn arm_immediate_byte_load_reads_one_byte_without_word_rotation() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE5D2_1001)); // LDRB r1, [r2, #1]
        memory.write_u8(0x0200_0001, 0xAB);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[2] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[1], 0xAB);
    }

    #[test]
    fn failed_arm_transfer_condition_does_not_reject_register_offset_encoding() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0x1592_1001)); // LDRNE r1, [r2, r1]
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= FLAG_Z;
        cpu.registers[1] = 7;
        cpu.registers[2] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[1], 7);
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn thumb_program_loads_literal_and_stores_to_ewram() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x4801, 0x2112, 0x6001, 0xE7FE, 0x0000, 0x0200]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        cpu.run_steps(&mut memory, 3).unwrap();

        assert_eq!(memory.read_u32(0x0200_0000), 0x12);
        assert_eq!(cpu.registers[0], 0x0200_0000);
        assert_eq!(cpu.registers[1], 0x12);
    }

    #[test]
    fn thumb_add_and_sub_update_registers_and_zero_flag() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x2002, 0x3003, 0x3805]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        cpu.run_steps(&mut memory, 3).unwrap();

        assert_eq!(cpu.registers[0], 0);
        assert_ne!(cpu.cpsr & FLAG_Z, 0);
        assert_ne!(cpu.cpsr & FLAG_C, 0);
    }

    #[test]
    fn thumb_three_operand_add_supports_register_and_immediate_operands() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x1C10, 0x1E50])); // ADDS r0, r2, #0; SUBS r0, r2, #1
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[2] = 0x7FFF_FFFF;

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0x7FFF_FFFF);
        assert_eq!(cpu.cpsr & FLAG_V, 0);

        cpu.registers[1] = 1;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0x7FFF_FFFE);
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn thumb_three_operand_register_add_updates_overflow() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x1888])); // ADDS r0, r1, r2
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[1] = 0x7FFF_FFFF;
        cpu.registers[2] = 1;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0x8000_0000);
        assert_ne!(cpu.cpsr & FLAG_N, 0);
        assert_ne!(cpu.cpsr & FLAG_V, 0);
    }

    #[test]
    fn thumb_cmp_immediate_updates_flags_without_changing_register() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x2900])); // CMP r1, #0
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[1] = 0;
        cpu.cpsr |= FLAG_C | FLAG_V;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[1], 0);
        assert_ne!(cpu.cpsr & FLAG_Z, 0);
        assert_ne!(cpu.cpsr & FLAG_C, 0);
        assert_eq!(cpu.cpsr & FLAG_V, 0);
    }

    #[test]
    fn thumb_register_orr_sets_result_and_nz_flags() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x4310])); // ORRS r0, r2
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x10;
        cpu.registers[2] = 0x8000_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0x8000_0010);
        assert_ne!(cpu.cpsr & FLAG_N, 0);
        assert_eq!(cpu.cpsr & FLAG_Z, 0);
    }

    #[test]
    fn thumb_register_and_and_eor_use_their_alu_slots() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x4010, 0x4050])); // ANDS r0, r2; EORS r0, r2
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0xF0;
        cpu.registers[2] = 0xCC;

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0xC0);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0x0C);
    }

    #[test]
    fn thumb_halfword_store_and_load_use_scaled_immediate_offset() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x8008, 0x8808])); // STRH r0, [r1, #0]; LDRH r0, [r1, #0]
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0xABCD;
        cpu.registers[1] = 0x0200_0002;

        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u16(0x0200_0002), 0xABCD);

        cpu.registers[0] = 0;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0xABCD);
    }

    #[test]
    fn thumb_byte_store_and_load_use_unscaled_immediate_offset() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x7008, 0x7808])); // STRB r0, [r1, #0]; LDRB r0, [r1, #0]
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0xABCD;
        cpu.registers[1] = 0x0200_0003;

        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u8(0x0200_0003), 0xCD);

        cpu.registers[0] = 0;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0xCD);
    }

    #[test]
    fn thumb_stmia_and_ldmia_use_ascending_words_and_write_back_base() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xC303, 0xCB03])); // STMIA r3!, {r0,r1}; LDMIA r3!, {r0,r1}
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x10;
        cpu.registers[1] = 0x20;
        cpu.registers[3] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u32(0x0200_0000), 0x10);
        assert_eq!(memory.read_u32(0x0200_0004), 0x20);
        assert_eq!(cpu.registers[3], 0x0200_0008);

        cpu.registers[0] = 0;
        cpu.registers[1] = 0;
        cpu.registers[3] = 0x0200_0000;
        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 0x10);
        assert_eq!(cpu.registers[1], 0x20);
        assert_eq!(cpu.registers[3], 0x0200_0008);
    }

    #[test]
    fn thumb_empty_register_list_transfers_pc_and_moves_base_by_sixteen_words() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xC000, 0xC800])); // STMIA r0!, {}; LDMIA r0!, {}
        memory.write_u32(0x0200_0040, ROM_BASE + 0x21);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();
        assert_eq!(memory.read_u32(0x0200_0000), ROM_BASE + 6);
        assert_eq!(cpu.registers[0], 0x0200_0040);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[15], ROM_BASE + 0x20);
        assert_eq!(cpu.registers[0], 0x0200_0080);
        assert!(cpu.is_thumb());
    }

    #[test]
    fn thumb_branch_uses_the_architectural_pc_plus_four() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xE7FE]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[15], ROM_BASE);
    }

    #[test]
    fn unsupported_thumb_opcode_returns_its_address_and_value() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDE00]));
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x1234;
        let original_registers = cpu.registers;

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction {
                address: ROM_BASE,
                state: ExecutionState::Thumb,
                opcode: 0xDE00,
            })
        );
        assert_eq!(cpu.registers, original_registers);
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
    fn arm_movs_updates_nz_and_preserves_cv() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[
            0x80, 0x04, 0xB0, 0xE3, // MOVS r0, #0x80000000
            0x00, 0x00, 0xB0, 0xE3, // MOVS r0, #0
        ]);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= FLAG_C | FLAG_V;

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0x8000_0000);
        assert_ne!(cpu.cpsr & FLAG_N, 0);
        assert_eq!(cpu.cpsr & FLAG_Z, 0);
        assert_eq!(cpu.cpsr & (FLAG_C | FLAG_V), FLAG_C | FLAG_V);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 0);
        assert_eq!(cpu.cpsr & (FLAG_N | FLAG_Z), FLAG_Z);
        assert_eq!(cpu.cpsr & (FLAG_C | FLAG_V), FLAG_C | FLAG_V);
    }

    #[test]
    fn arm_msr_cpsr_fc_accepts_user_mode_and_updates_flags() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x00, 0xF0, 0x29, 0xE1]); // MSR CPSR_fc, r0
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = CPSR_USER_MODE | (1 << 6) | FLAG_N | FLAG_C | FLAG_V;
        cpu.cpsr |= FLAG_Z;

        cpu.step(&mut memory).unwrap();

        assert_eq!(
            cpu.cpsr,
            CPSR_USER_MODE | (1 << 6) | FLAG_N | FLAG_C | FLAG_V
        );
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn arm_msr_cpsr_fc_switches_to_user_mode() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x00, 0xF0, 0x29, 0xE1]); // MSR CPSR_fc, r0
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = CPSR_USER_MODE;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_USER_MODE);
        assert_eq!(cpu.cpsr & (FLAG_N | FLAG_Z | FLAG_C | FLAG_V), 0);
        assert_eq!(cpu.registers[15], ROM_BASE + 4);
    }

    #[test]
    fn arm_msr_mode_switch_restores_separate_supervisor_stack_bank() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[
            0x00, 0xF0, 0x29, 0xE1, // MSR CPSR_fc, r0
            0x00, 0xF0, 0x29, 0xE1, // MSR CPSR_fc, r0
            0x00, 0xF0, 0x29, 0xE1, // MSR CPSR_fc, r0
        ]);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = CPSR_SVC_MODE;

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SVC_MODE);
        assert_eq!(cpu.registers[13], HLE_SVC_STACK);
        cpu.registers[13] = 0x0300_7FD0;

        cpu.registers[0] = CPSR_SYSTEM_MODE;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SYSTEM_MODE);
        assert_eq!(cpu.registers[13], 0x0300_7F00);

        cpu.registers[0] = CPSR_SVC_MODE;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SVC_MODE);
        assert_eq!(cpu.registers[13], 0x0300_7FD0);
    }

    #[test]
    fn arm_msr_rejects_unmodeled_mode_changes_without_mutating_cpsr() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x00, 0xF0, 0x29, 0xE1]); // MSR CPSR_fc, r0
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = FLAG_N;
        let original_cpsr = cpu.cpsr;

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedArmInstruction {
                address: ROM_BASE,
                state: ExecutionState::Arm,
                opcode: 0xE129_F000,
            })
        ));
        assert_eq!(cpu.cpsr, original_cpsr);
        assert_eq!(cpu.registers[15], ROM_BASE);
    }

    #[test]
    fn arm_bx_reads_pc_as_instruction_address_plus_eight() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x1F, 0xFF, 0x2F, 0xE1]); // BX pc
        let mut cpu = ThumbCpu::boot_hle();

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[15], ROM_BASE + 8);
        assert!(!cpu.is_thumb());
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
    fn unsupported_arm_opcode_reports_state_and_preserves_cpu_and_memory() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x90, 0x00, 0xA0, 0xE1]); // unsupported multiply-family encoding
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = 0x1234;
        let original_registers = cpu.registers;
        let original_cpsr = cpu.cpsr;
        let original_memory = memory.clone();

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedArmInstruction {
                address: ROM_BASE,
                state: ExecutionState::Arm,
                opcode: 0xE1A0_0090,
            })
        );
        assert_eq!(cpu.registers, original_registers);
        assert_eq!(cpu.cpsr, original_cpsr);
        assert_eq!(memory.cartridge, original_memory.cartridge);
        assert_eq!(memory.ewram, original_memory.ewram);
    }

    #[test]
    fn arm_nv_encoding_is_reported_as_unsupported_without_advancing_pc() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x10, 0xFF, 0x2F, 0xF1]); // NV BX pc
        let mut cpu = ThumbCpu::boot_hle();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedArmInstruction {
                state: ExecutionState::Arm,
                ..
            })
        ));
        assert_eq!(cpu.registers[15], ROM_BASE);
    }

    #[test]
    fn thumb_byte_store_writes_only_the_low_byte() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x7008]));
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x1234_56AB;
        cpu.registers[1] = 0x0200_0000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(memory.read_u8(0x0200_0000), 0xAB);
        assert_eq!(memory.read_u8(0x0200_0001), 0);
    }

    #[test]
    fn hle_boot_starts_in_arm_system_mode_with_iwram_stack() {
        let cpu = ThumbCpu::boot_hle();

        assert!(!cpu.is_thumb());
        assert_eq!(cpu.registers[13], 0x0300_7F00);
        assert_eq!(cpu.registers[15], ROM_BASE);
        assert_eq!(cpu.cpsr & 0x1F, CPSR_SYSTEM_MODE);
    }

    #[test]
    fn arm_branch_uses_pc_plus_eight_and_executes_arm_instruction() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[
            0x00, 0x00, 0x00, 0xEA, // B 0x08000008
            0x00, 0x00, 0xA0, 0xE1, // skipped instruction
            0x2A, 0x00, 0xA0, 0xE3, // MOV r0, #42
        ]);
        let mut cpu = ThumbCpu::boot_hle();

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[15], ROM_BASE + 8);
        assert!(!cpu.is_thumb());
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 42);
        assert_eq!(cpu.registers[15], ROM_BASE + 12);
    }

    #[test]
    fn arm_bx_switches_to_thumb_and_aligns_the_target() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[
            0x10, 0xFF, 0x2F, 0xE1, // BX r0
            0x2A, 0x20, // MOVS r0, #42
        ]);
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = ROM_BASE + 5;

        cpu.step(&mut memory).unwrap();
        assert!(cpu.is_thumb());
        assert_eq!(cpu.registers[15], ROM_BASE + 4);

        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 42);
        assert_eq!(cpu.registers[15], ROM_BASE + 6);
    }
}
