use super::*;

impl ThumbCpu {
    pub(super) fn unsupported_arm(address: u32, opcode: u32) -> CpuError {
        CpuError::UnsupportedArmInstruction {
            address,
            state: ExecutionState::Arm,
            opcode,
        }
    }

    pub(super) fn step_arm(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
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

        // ARM7TDMI decodes BX/MRS/MSR from bits 27-20 and 7-4; the SBO/SBZ fields are ignored.
        if opcode & 0x0FF0_00F0 == 0x0120_0010 {
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
        // SWP's SBZ bits 11-8 are not decoded, as for BX/MRS/MSR above.
        if opcode & 0x0FB0_00F0 == 0x0100_0090 {
            return self.arm_swap(memory, opcode, address);
        }
        if opcode & 0x0E00_0090 == 0x0000_0090 && opcode & 0x60 != 0 {
            return self.arm_halfword_transfer(memory, opcode, address);
        }
        if opcode & 0x0FB0_00F0 == 0x0100_0000 {
            return self.arm_mrs(opcode, address);
        }
        if opcode & 0x0FB0_00F0 == 0x0120_0000 || opcode & 0x0FB0_0000 == 0x0320_0000 {
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

    pub(super) fn arm_data_processing(
        &mut self,
        opcode: u32,
        address: u32,
    ) -> Result<(), CpuError> {
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

    pub(super) fn arm_multiply(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
        let accumulate = opcode & (1 << 21) != 0;
        let set_flags = opcode & (1 << 20) != 0;
        let destination = ((opcode >> 16) & 0xF) as usize;
        let operands = [
            (opcode & 0xF) as usize,
            ((opcode >> 8) & 0xF) as usize,
            ((opcode >> 12) & 0xF) as usize,
        ];
        // MUL does not read Rn (bits 15-12, SBZ), so only MLA rejects it as PC.
        let read_operands = if accumulate {
            &operands[..]
        } else {
            &operands[..2]
        };
        if destination == 15 || read_operands.contains(&15) {
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

    pub(super) fn arm_multiply_long(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
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

    pub(super) fn arm_swap(
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

    pub(super) fn arm_mrs(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
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

    pub(super) fn arm_msr(&mut self, opcode: u32, address: u32) -> Result<(), CpuError> {
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

    pub(super) fn arm_single_transfer(
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

    pub(super) fn arm_halfword_transfer(
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

    pub(super) fn arm_block_transfer(
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
    pub(super) fn write_register_or_pc(&mut self, register: usize, value: u32) {
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

    pub(super) fn branch_exchange(&mut self, target: u32) {
        if target & 1 != 0 {
            self.cpsr |= CPSR_THUMB;
            self.registers[15] = target & !1;
        } else {
            self.cpsr &= !CPSR_THUMB;
            self.registers[15] = target & !3;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::test_util::*;

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
    fn arm_bx_mrs_msr_ignore_sbo_and_sbz_fields() {
        // BX r0 with SBO bits 19-8 cleared.
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE120_0010));
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[0] = ROM_BASE + 0x21;
        cpu.step(&mut memory).unwrap();
        assert!(cpu.is_thumb());
        assert_eq!(cpu.registers[15], ROM_BASE + 0x20);

        // MRS r2, CPSR with SBO bits 19-16 cleared and SBZ bits 11-0 set.
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE100_2F0F));
        let mut cpu = ThumbCpu::boot_hle();
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[2], cpu.cpsr);

        // MSR CPSR_f, r0 / MSR CPSR_f, #0xF0000000 with SBO bits 15-12 cleared.
        for opcode in [0xE128_0F00u32, 0xE328_020F] {
            let mut memory = MemoryMap::new();
            memory.load_cartridge(&arm_word(opcode));
            let mut cpu = ThumbCpu::boot_hle();
            cpu.registers[0] = 0xF000_0000;
            cpu.step(&mut memory).unwrap();
            assert_eq!(cpu.cpsr & 0xF000_0000, 0xF000_0000, "opcode {opcode:08X}");
        }
    }

    #[test]
    fn arm_mul_ignores_sbz_rn_field() {
        // MUL r0, r1, r2 with Rn (bits 15-12) = 15; only MLA reads Rn.
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xE000_F291));
        let mut cpu = ThumbCpu::boot_hle();
        cpu.registers[1] = 6;
        cpu.registers[2] = 7;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[0], 42);
    }

    #[test]
    fn arm_swp_ignores_sbz_bits_11_to_8() {
        // SWP r0, r1, [r2] and SWPB r0, r1, [r2] with bits 11-8 set.
        for (opcode, expected_loaded, expected_stored) in [
            (0xE102_0F91u32, 0x1122_3344u32, 0xAABB_CCDDu32),
            (0xE142_0F91, 0x44, 0x1122_33DD),
        ] {
            let mut memory = MemoryMap::new();
            memory.load_cartridge(&arm_word(opcode));
            memory.write_u32(0x0300_0000, 0x1122_3344);
            let mut cpu = ThumbCpu::boot_hle();
            cpu.registers[1] = 0xAABB_CCDD;
            cpu.registers[2] = 0x0300_0000;

            cpu.step(&mut memory).unwrap();

            assert_eq!(cpu.registers[0], expected_loaded, "opcode {opcode:08X}");
            assert_eq!(
                memory.read_u32(0x0300_0000),
                expected_stored,
                "opcode {opcode:08X}"
            );
        }
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
}
