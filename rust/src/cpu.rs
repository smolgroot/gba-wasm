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
    UnsupportedBiosCall {
        address: u32,
        state: ExecutionState,
        service: u8,
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

        if opcode & 0x0F00_0000 == 0x0F00_0000 {
            if self.condition_passed(condition) {
                self.execute_bios_call(
                    memory,
                    address,
                    ExecutionState::Arm,
                    (opcode & 0xFF) as u8,
                )?;
            }
            return Ok(());
        }

        if opcode & 0x0FBF_0FFF == 0x010F_0000 {
            if self.condition_passed(condition) {
                let destination = ((opcode >> 12) & 0xF) as usize;
                if destination == 15 {
                    return Err(CpuError::UnsupportedArmInstruction {
                        address,
                        state: ExecutionState::Arm,
                        opcode,
                    });
                }
                self.registers[destination] = self.cpsr;
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

        if opcode & 0x0E00_0090 == 0x0000_0090 {
            if !self.condition_passed(condition) {
                return Ok(());
            }

            let pre_index = opcode & (1 << 24) != 0;
            let add_offset = opcode & (1 << 23) != 0;
            let immediate_offset = opcode & (1 << 22) != 0;
            let writeback = opcode & (1 << 21) != 0;
            let load = opcode & (1 << 20) != 0;
            let base_register = ((opcode >> 16) & 0xF) as usize;
            let destination = ((opcode >> 12) & 0xF) as usize;
            let signed = opcode & (1 << 6) != 0;
            let halfword = opcode & (1 << 5) != 0;

            if (!signed && !halfword) || (!load && signed) {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
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
                let value = match (signed, halfword) {
                    (false, true) => u32::from(memory.read_u16(transfer_address & !1)),
                    (true, false) => i32::from(memory.read_u8(transfer_address) as i8) as u32,
                    (true, true) => i32::from(memory.read_u16(transfer_address & !1) as i16) as u32,
                    (false, false) => unreachable!(),
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
                memory.write_u16(transfer_address & !1, value as u16);
            }

            if !pre_index || writeback {
                self.registers[base_register] = adjusted_base;
            }
            return Ok(());
        }

        if opcode & 0x0C00_0000 == 0 && opcode & (1 << 25) != 0 {
            if !self.condition_passed(condition) {
                return Ok(());
            }

            let operation = (opcode >> 21) & 0xF;
            let set_flags = opcode & (1 << 20) != 0;
            let destination = ((opcode >> 12) & 0xF) as usize;
            let source_register = ((opcode >> 16) & 0xF) as usize;
            let rotation = ((opcode >> 8) & 0xF) * 2;
            let immediate = opcode & 0xFF;
            let operand = immediate.rotate_right(rotation);
            let shifter_carry = if rotation == 0 {
                self.cpsr & FLAG_C != 0
            } else {
                operand & FLAG_N != 0
            };
            let source = self.read_register(source_register, ExecutionState::Arm, address);
            let carry_in = u32::from(self.cpsr & FLAG_C != 0);
            let is_test = matches!(operation, 8..=11);

            if is_test && !set_flags {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }
            if destination == 15 && set_flags {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }

            let (result, arithmetic_flags) = match operation {
                0 => (source & operand, None),
                1 => (source ^ operand, None),
                2 | 10 => {
                    let result = source.wrapping_sub(operand);
                    (result, Some((true, source, operand, 0)))
                }
                3 => {
                    let result = operand.wrapping_sub(source);
                    (result, Some((true, operand, source, 0)))
                }
                4 | 11 => {
                    let result = source.wrapping_add(operand);
                    (result, Some((false, source, operand, 0)))
                }
                5 => {
                    let result = source.wrapping_add(operand).wrapping_add(carry_in);
                    (result, Some((false, source, operand, carry_in)))
                }
                6 => {
                    let borrow = 1 - carry_in;
                    let result = source.wrapping_sub(operand).wrapping_sub(borrow);
                    (result, Some((true, source, operand, borrow)))
                }
                7 => {
                    let borrow = 1 - carry_in;
                    let result = operand.wrapping_sub(source).wrapping_sub(borrow);
                    (result, Some((true, operand, source, borrow)))
                }
                8 => (source & operand, None),
                9 => (source ^ operand, None),
                12 => (source | operand, None),
                13 => (operand, None),
                14 => (source & !operand, None),
                15 => (!operand, None),
                _ => unreachable!(),
            };

            if set_flags {
                if let Some((subtract, left, right, adjustment)) = arithmetic_flags {
                    self.set_arithmetic_flags(subtract, left, right, adjustment, result);
                } else {
                    self.set_nz(result);
                    self.cpsr = (self.cpsr & !FLAG_C) | if shifter_carry { FLAG_C } else { 0 };
                }
            }

            if !is_test {
                if destination == 15 {
                    self.registers[15] = result & !3;
                } else {
                    self.registers[destination] = result;
                }
            }
            return Ok(());
        }

        if opcode & 0x0C00_0000 == 0 && opcode & (1 << 25) == 0 {
            if !self.condition_passed(condition) {
                return Ok(());
            }
            if opcode & (1 << 4) != 0 && opcode & (1 << 7) != 0 {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }

            let operation = (opcode >> 21) & 0xF;
            let set_flags = opcode & (1 << 20) != 0;
            let destination = ((opcode >> 12) & 0xF) as usize;
            let source_register = ((opcode >> 16) & 0xF) as usize;
            let value_register = (opcode & 0xF) as usize;
            let shift_type = (opcode >> 5) & 0x3;
            let value = self.read_register(value_register, ExecutionState::Arm, address);
            let source = self.read_register(source_register, ExecutionState::Arm, address);
            let (operand, shifter_carry) = if opcode & (1 << 4) == 0 {
                let shift_amount = (opcode >> 7) & 0x1F;
                shift_immediate(value, shift_type, shift_amount, self.cpsr & FLAG_C != 0)
            } else {
                let shift_register_index = ((opcode >> 8) & 0xF) as usize;
                let shift_amount =
                    self.read_register(shift_register_index, ExecutionState::Arm, address);
                shift_register(value, shift_amount, shift_type, self.cpsr & FLAG_C != 0)
            };
            let carry_in = u32::from(self.cpsr & FLAG_C != 0);
            let is_test = matches!(operation, 8..=11);

            if is_test && !set_flags {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }
            if destination == 15 && set_flags {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }

            let (result, arithmetic_flags) = match operation {
                0 => (source & operand, None),
                1 => (source ^ operand, None),
                2 | 10 => {
                    let result = source.wrapping_sub(operand);
                    (result, Some((true, source, operand, 0)))
                }
                3 => {
                    let result = operand.wrapping_sub(source);
                    (result, Some((true, operand, source, 0)))
                }
                4 | 11 => {
                    let result = source.wrapping_add(operand);
                    (result, Some((false, source, operand, 0)))
                }
                5 => {
                    let result = source.wrapping_add(operand).wrapping_add(carry_in);
                    (result, Some((false, source, operand, carry_in)))
                }
                6 => {
                    let borrow = 1 - carry_in;
                    let result = source.wrapping_sub(operand).wrapping_sub(borrow);
                    (result, Some((true, source, operand, borrow)))
                }
                7 => {
                    let borrow = 1 - carry_in;
                    let result = operand.wrapping_sub(source).wrapping_sub(borrow);
                    (result, Some((true, operand, source, borrow)))
                }
                8 => (source & operand, None),
                9 => (source ^ operand, None),
                12 => (source | operand, None),
                13 => (operand, None),
                14 => (source & !operand, None),
                15 => (!operand, None),
                _ => unreachable!(),
            };

            if set_flags {
                if let Some((subtract, left, right, adjustment)) = arithmetic_flags {
                    self.set_arithmetic_flags(subtract, left, right, adjustment, result);
                } else {
                    self.set_nz(result);
                    self.cpsr = (self.cpsr & !FLAG_C) | if shifter_carry { FLAG_C } else { 0 };
                }
            }

            if !is_test {
                if destination == 15 {
                    self.registers[15] = result & !3;
                } else {
                    self.registers[destination] = result;
                }
            }
            return Ok(());
        }

        if opcode & 0x0C00_0000 == 0x0400_0000 {
            if !self.condition_passed(condition) {
                return Ok(());
            }
            if opcode & (1 << 25) != 0 && opcode & (1 << 4) != 0 {
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
            let offset = if opcode & (1 << 25) == 0 {
                opcode & 0xFFF
            } else {
                let value_register = (opcode & 0xF) as usize;
                let value = self.read_register(value_register, ExecutionState::Arm, address);
                let shift_type = (opcode >> 5) & 0x3;
                let shift_amount = (opcode >> 7) & 0x1F;
                shift_immediate(value, shift_type, shift_amount, self.cpsr & FLAG_C != 0).0
            };
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

        if opcode & 0x0E00_0000 == 0x0800_0000 {
            if !self.condition_passed(condition) {
                return Ok(());
            }

            let pre_index = opcode & (1 << 24) != 0;
            let increment = opcode & (1 << 23) != 0;
            let user_mode_registers = opcode & (1 << 22) != 0;
            let writeback = opcode & (1 << 21) != 0;
            let load = opcode & (1 << 20) != 0;
            let base_register = ((opcode >> 16) & 0xF) as usize;
            let register_list = opcode & 0xFFFF;

            if user_mode_registers
                || register_list == 0
                || (writeback
                    && (base_register == 15 || (load && register_list & (1 << base_register) != 0)))
            {
                return Err(CpuError::UnsupportedArmInstruction {
                    address,
                    state: ExecutionState::Arm,
                    opcode,
                });
            }

            let base = self.read_register(base_register, ExecutionState::Arm, address);
            let register_count = register_list.count_ones();
            let transfer_address = match (increment, pre_index) {
                (true, false) => base,
                (true, true) => base.wrapping_add(4),
                (false, false) => base.wrapping_sub((register_count - 1) * 4),
                (false, true) => base.wrapping_sub(register_count * 4),
            };
            let updated_base = if increment {
                base.wrapping_add(register_count * 4)
            } else {
                base.wrapping_sub(register_count * 4)
            };

            let mut transfer_address = transfer_address;
            for register in 0..16 {
                if register_list & (1 << register) == 0 {
                    continue;
                }

                if load {
                    let value = memory.read_u32(transfer_address & !3);
                    if register == 15 {
                        self.registers[15] = value & !3;
                    } else {
                        self.registers[register] = value;
                    }
                } else {
                    let value = if register == 15 {
                        address.wrapping_add(12)
                    } else {
                        self.registers[register]
                    };
                    memory.write_u32(transfer_address & !3, value);
                }
                transfer_address = transfer_address.wrapping_add(4);
            }

            if writeback {
                self.registers[base_register] = updated_base;
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
            let literal_address = self.read_register(15, ExecutionState::Thumb, address);
            let literal_address = literal_address.wrapping_add(u32::from(opcode & 0x00FF) * 4);
            self.registers[destination] = memory.read_u32(literal_address);
            return Ok(());
        }

        if opcode & 0xF000 == 0x9000 {
            let load = opcode & 0x0800 != 0;
            let destination = usize::from((opcode >> 8) & 0x7);
            let offset = u32::from(opcode & 0x00FF) * 4;
            let transfer_address = self.registers[13].wrapping_add(offset);
            if load {
                self.registers[destination] = memory.read_u32(transfer_address & !3);
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
                let target = memory.read_u32(address);
                if target & 1 != 0 {
                    self.cpsr |= CPSR_THUMB;
                    self.registers[15] = target & !1;
                } else {
                    self.cpsr &= !CPSR_THUMB;
                    self.registers[15] = target & !3;
                }
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
                self.registers[source_or_destination] = memory.read_u32(address);
            } else {
                memory.write_u32(address, self.registers[source_or_destination]);
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
                self.registers[source_or_destination] =
                    u32::from(memory.read_u16(transfer_address & !1));
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
            let register_list = u32::from(opcode & 0x00FF);
            if register_list == 0 {
                return Err(CpuError::UnsupportedInstruction {
                    address,
                    state: ExecutionState::Thumb,
                    opcode,
                });
            }

            let initial_address = self.registers[base_register];
            let register_count = register_list.count_ones();
            let updated_base = initial_address.wrapping_add(register_count * 4);
            let mut transfer_address = initial_address;
            for register in 0..8 {
                if register_list & (1 << register) == 0 {
                    continue;
                }
                if load {
                    self.registers[register] = memory.read_u32(transfer_address & !3);
                } else {
                    memory.write_u32(transfer_address & !3, self.registers[register]);
                }
                transfer_address = transfer_address.wrapping_add(4);
            }

            if !load || register_list & (1 << base_register) == 0 {
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
                    if right & 1 != 0 {
                        self.cpsr |= CPSR_THUMB;
                        self.registers[15] = right & !1;
                    } else {
                        self.cpsr &= !CPSR_THUMB;
                        self.registers[15] = right & !3;
                    }
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

            if write_result {
                self.registers[destination] = result;
                if !matches!(operation, 5 | 6 | 8 | 9 | 10 | 11) {
                    self.set_nz(result);
                }
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
        if flags & 0xE0 != 0 {
            memory.io.fill(0);
        }
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
        let word_count = (control & 0x001F_FFFF).wrapping_mul(8);
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

    fn execute_bios_call(
        &self,
        memory: &mut MemoryMap,
        address: u32,
        state: ExecutionState,
        service: u8,
    ) -> Result<(), CpuError> {
        match service {
            0x01 if self.registers[0] & !0xFF == 0 => {
                self.register_ram_reset(memory, self.registers[0]);
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
        memory.load_cartridge(&arm_word(0xEF00_0001)); // SWI RegisterRamReset
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
        assert!(memory.io.iter().all(|byte| *byte == 0));
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

    #[test]
    fn unsupported_thumb_swi_is_reported_explicitly() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF02]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedBiosCall {
                address: ROM_BASE,
                state: ExecutionState::Thumb,
                service: 2,
            })
        );
        assert_eq!(cpu.registers[15], ROM_BASE);
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
    fn thumb_multiple_transfer_rejects_an_empty_register_list() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xC000]));
        let mut cpu = ThumbCpu::new(ROM_BASE);

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedInstruction { opcode: 0xC000, .. })
        ));
        assert_eq!(cpu.registers[15], ROM_BASE);
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
