use crate::memory::MemoryMap;
use std::fmt;

pub const ROM_BASE: u32 = 0x0800_0000;

const CPSR_THUMB: u32 = 1 << 5;
const CPSR_SYSTEM_MODE: u32 = 0x1F;
const FLAG_N: u32 = 1 << 31;
const FLAG_Z: u32 = 1 << 30;
const FLAG_C: u32 = 1 << 29;
const FLAG_V: u32 = 1 << 28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuError {
    EmptyCartridge,
    UnsupportedInstruction { address: u32, opcode: u16 },
    UnsupportedArmInstruction { address: u32, opcode: u32 },
}

impl fmt::Display for CpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCartridge => write!(formatter, "cartridge ROM is empty"),
            Self::UnsupportedInstruction { address, opcode } => {
                write!(
                    formatter,
                    "unsupported Thumb opcode 0x{opcode:04X} at 0x{address:08X}"
                )
            }
            Self::UnsupportedArmInstruction { address, opcode } => {
                write!(
                    formatter,
                    "unsupported ARM opcode 0x{opcode:08X} at 0x{address:08X}"
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThumbCpu {
    pub registers: [u32; 16],
    pub cpsr: u32,
}

impl ThumbCpu {
    pub fn new(entry: u32) -> Self {
        let mut registers = [0; 16];
        registers[15] = entry;
        Self {
            registers,
            cpsr: CPSR_SYSTEM_MODE | CPSR_THUMB,
        }
    }

    pub fn boot_hle() -> Self {
        let mut registers = [0; 16];
        registers[13] = 0x0300_7F00;
        registers[15] = ROM_BASE;
        Self {
            registers,
            cpsr: CPSR_SYSTEM_MODE,
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
        if self.is_thumb() {
            self.step_thumb(memory)
        } else {
            self.step_arm(memory)
        }
    }

    fn step_arm(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
        let address = self.registers[15];
        let opcode = memory.read_u32(address);
        self.registers[15] = address.wrapping_add(4);
        let condition = (opcode >> 28) as u8;

        if opcode & 0x0FFF_FFF0 == 0x012F_FF10 {
            if self.condition_passed(condition) {
                let target = self.registers[(opcode & 0xF) as usize];
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

        if opcode & 0x0E00_0000 == 0x0A00_0000 {
            if condition == 0xF {
                return Err(CpuError::UnsupportedArmInstruction { address, opcode });
            }
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
            if condition == 0xF {
                return Err(CpuError::UnsupportedArmInstruction { address, opcode });
            }
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

        Err(CpuError::UnsupportedArmInstruction { address, opcode })
    }

    fn step_thumb(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
        let address = self.registers[15];
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
            let literal_address = address.wrapping_add(4) & !3;
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
                return Err(CpuError::UnsupportedInstruction { address, opcode });
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

        Err(CpuError::UnsupportedInstruction { address, opcode })
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

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction {
                address: ROM_BASE,
                opcode: 0x0000,
            })
        );
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
