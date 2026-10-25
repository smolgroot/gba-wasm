use crate::memory::MemoryMap;
use std::fmt;

pub const ROM_BASE: u32 = 0x0800_0000;

const CPSR_THUMB: u32 = 1 << 5;
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
}

impl ThumbCpu {
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
        }
    }

    pub fn boot_hle() -> Self {
        let mut registers = [0; 16];
        registers[13] = 0x0300_7F00;
        registers[15] = ROM_BASE;
        let mut user_bank = [0; 7];
        user_bank[5] = registers[13];
        Self {
            registers,
            cpsr: CPSR_SYSTEM_MODE,
            user_bank,
            fiq_bank: [0; 7],
            irq_bank: [0; 2],
            svc_bank: [0; 2],
            abort_bank: [0; 2],
            undefined_bank: [0; 2],
        }
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
        let faulting_pc = self.registers[15];
        let result = if self.is_thumb() {
            self.step_thumb(memory)
        } else {
            self.step_arm(memory)
        };
        if result.is_err() {
            self.registers[15] = faulting_pc;
        }
        result
    }

    fn step_arm(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
        let address = self.registers[15] & !3;
        let opcode = memory.read_u32(address);
        self.registers[15] = address.wrapping_add(4);
        let condition = (opcode >> 28) as u8;
        if condition == 0xF {
            return Err(CpuError::UnsupportedArmInstruction {
                address,
                state: ExecutionState::Arm,
                opcode,
            });
        }

        if opcode & 0x0FFF_FFF0 == 0x012F_FF10 {
            if self.condition_passed(condition) {
                let target =
                    self.read_register((opcode & 0xF) as usize, ExecutionState::Arm, address);
                if target & 1 != 0 {
                    self.cpsr |= CPSR_THUMB;
                    self.registers[15] = target & !1;
                } else {
                    self.cpsr &= !CPSR_THUMB;
                    self.registers[15] = target & !3;
                }
            }
            return Ok(());
        }

        if opcode & 0x0FB0_FFF0 == 0x0120_F000 {
            if self.condition_passed(condition) {
                let writes_spsr = opcode & (1 << 22) != 0;
                let field_mask = (opcode >> 16) & 0xF;
                if writes_spsr || field_mask == 0 {
                    return Err(CpuError::UnsupportedArmInstruction {
                        address,
                        state: ExecutionState::Arm,
                        opcode,
                    });
                }

                let source_register = (opcode & 0xF) as usize;
                let value = self.read_register(source_register, ExecutionState::Arm, address);
                let mut write_mask = 0;
                for field in 0..4 {
                    if field_mask & (1 << field) != 0 {
                        write_mask |= 0xFF << (field * 8);
                    }
                }

                let updated_cpsr = (self.cpsr & !write_mask) | (value & write_mask);
                let control_written = field_mask & 1 != 0;
                let updated_mode = updated_cpsr & CPSR_MODE_MASK;
                if control_written
                    && (!Self::is_valid_mode(updated_mode)
                        || (updated_cpsr ^ self.cpsr) & CPSR_THUMB != 0
                        || self.cpsr & CPSR_MODE_MASK == CPSR_USER_MODE)
                {
                    return Err(CpuError::UnsupportedArmInstruction {
                        address,
                        state: ExecutionState::Arm,
                        opcode,
                    });
                }

                if control_written && updated_mode != self.cpsr & CPSR_MODE_MASK {
                    self.switch_mode(updated_mode);
                }
                self.cpsr = updated_cpsr;
            }
            return Ok(());
        }

        if opcode & 0x0C00_0000 == 0x0400_0000 {
            if !self.condition_passed(condition) {
                return Ok(());
            }
            if opcode & (1 << 25) != 0 {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }
            let pre_index = opcode & (1 << 24) != 0;
            let add_offset = opcode & (1 << 23) != 0;
            let byte_transfer = opcode & (1 << 22) != 0;
            let writeback = opcode & (1 << 21) != 0;
            let load = opcode & (1 << 20) != 0;
            let base_register = ((opcode >> 16) & 0xF) as usize;
            let destination = ((opcode >> 12) & 0xF) as usize;
            let base = self.read_register(base_register, ExecutionState::Arm, address);
            let offset = opcode & 0xFFF;
            let adjusted_base = if add_offset {
                base.wrapping_add(offset)
            } else {
                base.wrapping_sub(offset)
            };
            let transfer_address = if pre_index { adjusted_base } else { base };

            if (!pre_index || writeback)
                && (base_register == 15 || (load && destination == base_register))
            {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }

            if load {
                let value = if byte_transfer {
                    u32::from(memory.read_u8(transfer_address))
                } else {
                    let aligned_address = transfer_address & !3;
                    memory
                        .read_u32(aligned_address)
                        .rotate_right((transfer_address & 3) * 8)
                };
                if destination == 15 {
                    self.registers[15] = value & !3;
                } else {
                    self.registers[destination] = value;
                }
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
            }

            if !pre_index || writeback {
                self.registers[base_register] = adjusted_base;
            }
            return Ok(());
        }

        if opcode & 0x0E00_0000 == 0x0A00_0000 {
            if self.condition_passed(condition) {
                if opcode & (1 << 24) != 0 {
                    self.registers[14] = address.wrapping_add(4);
                }
                let offset = sign_extend(opcode & 0x00FF_FFFF, 24) << 2;
                self.registers[15] = address.wrapping_add(8).wrapping_add(offset);
            }
            return Ok(());
        }

        if opcode & 0x0FE0_0000 == 0x03A0_0000 {
            if self.condition_passed(condition) {
                let destination = ((opcode >> 12) & 0xF) as usize;
                let immediate = opcode & 0xFF;
                let rotation = ((opcode >> 8) & 0xF) * 2;
                let value = immediate.rotate_right(rotation);
                self.registers[destination] = value;
                if opcode & (1 << 20) != 0 {
                    self.set_nz(value);
                }
            }
            return Ok(());
        }

        Err(CpuError::UnsupportedArmInstruction {
            address,
            state: ExecutionState::Arm,
            opcode,
        })
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
            let literal_address = self.read_register(15, ExecutionState::Thumb, address);
            let literal_address = literal_address.wrapping_add(u32::from(opcode & 0x00FF) * 4);
            self.registers[destination] = memory.read_u32(literal_address);
            return Ok(());
        }

        if opcode & 0xF000 == 0x6000 {
            let is_load = opcode & 0x0800 != 0;
            let base = usize::from((opcode >> 3) & 0x7);
            let source_or_destination = usize::from(opcode & 0x7);
            let offset = u32::from((opcode >> 6) & 0x1F) * 4;
            let address = self.registers[base].wrapping_add(offset);

            if is_load {
                self.registers[source_or_destination] = memory.read_u32(address);
            } else {
                memory.write_u32(address, self.registers[source_or_destination]);
            }
            return Ok(());
        }

        if opcode & 0xF000 == 0xD000 {
            let condition = ((opcode >> 8) & 0xF) as u8;
            if condition >= 0xE {
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
            ExecutionState::Thumb => instruction_address.wrapping_add(4) & !3,
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
}

fn sign_extend(value: u32, bits: u32) -> u32 {
    ((value << (32 - bits)) as i32 >> (32 - bits)) as u32
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
        memory.load_cartridge(&program(&[0x0000]));
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[0] = 0x1234;
        let original_registers = cpu.registers;

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction {
                address: ROM_BASE,
                state: ExecutionState::Thumb,
                opcode: 0x0000,
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
        assert_eq!(cpu.registers[13], 0);
        cpu.registers[13] = 0x0300_7FE0;

        cpu.registers[0] = CPSR_SYSTEM_MODE;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SYSTEM_MODE);
        assert_eq!(cpu.registers[13], 0x0300_7F00);

        cpu.registers[0] = CPSR_SVC_MODE;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.cpsr & CPSR_MODE_MASK, CPSR_SVC_MODE);
        assert_eq!(cpu.registers[13], 0x0300_7FE0);
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
        memory.load_cartridge(&[0x00, 0x00, 0xA0, 0xE1]);
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
                opcode: 0xE1A0_0000,
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
    fn unsupported_byte_store_is_not_misdecoded_as_word_store() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x7000]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction { opcode: 0x7000, .. })
        ));
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
