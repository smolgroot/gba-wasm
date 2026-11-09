use super::*;

impl ThumbCpu {
    pub(super) fn step_thumb(&mut self, memory: &mut MemoryMap) -> Result<(), CpuError> {
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
            // PUSH is STMDB sp!: an empty list stores PC (as Thumb STMIA) and moves SP by 16 words.
            if register_count == 0 {
                let new_stack = self.registers[13].wrapping_sub(0x40);
                memory.write_u32(new_stack & !3, address.wrapping_add(6));
                self.registers[13] = new_stack;
                return Ok(());
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
            // POP is LDMIA sp!: an empty list loads PC (staying in Thumb) and moves SP by 16 words.
            if register_count == 0 {
                self.registers[15] = memory.read_u32(self.registers[13] & !3) & !1;
                self.registers[13] = self.registers[13].wrapping_add(0x40);
                return Ok(());
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
                // ARM7TDMI ignores H1 and the Rd field for BX (no BLX on ARMv4T).
                3 => self.branch_exchange(right),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::test_util::*;

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
    fn thumb_bx_ignores_h1_and_rd_bits() {
        // BX r0 with Rd=1, BX r0 with H1+Rd=7, BX lr with H1+Rd=7.
        for opcode in [0x4701u16, 0x4787, 0x47F7] {
            let mut memory = MemoryMap::new();
            memory.load_cartridge(&program(&[opcode]));
            let mut cpu = ThumbCpu::new(ROM_BASE);
            let source = usize::from((opcode >> 3) & 0xF);
            cpu.registers[source] = ROM_BASE + 0x41;

            cpu.step(&mut memory).unwrap();

            assert!(cpu.is_thumb(), "opcode {opcode:04X}");
            assert_eq!(cpu.registers[15], ROM_BASE + 0x40, "opcode {opcode:04X}");
            assert_eq!(cpu.registers[7], 0, "Rd bits must not be written");
        }
    }

    #[test]
    fn thumb_bx_with_rd_bits_from_rom_hack_branches_to_arm() {
        // 0x470A (BX r1, Rd=2) stopped Pokémon Silver Moon at 0x080A19EA.
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x470A]));
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[1] = ROM_BASE + 0x80;

        cpu.step(&mut memory).unwrap();

        assert!(!cpu.is_thumb());
        assert_eq!(cpu.registers[15], ROM_BASE + 0x80);
        assert_eq!(cpu.registers[2], 0);
    }

    #[test]
    fn thumb_push_empty_list_stores_pc_and_moves_sp_down_sixteen_words() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xB400])); // PUSH {}
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[13] = 0x0300_0100;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[13], 0x0300_00C0);
        assert_eq!(memory.read_u32(0x0300_00C0), ROM_BASE + 6);
        assert_eq!(cpu.registers[15], ROM_BASE + 2);
    }

    #[test]
    fn thumb_pop_empty_list_loads_pc_and_moves_sp_up_sixteen_words() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xBC00])); // POP {}
        memory.write_u32(0x0300_0100, ROM_BASE + 0x41);
        let mut cpu = ThumbCpu::new(ROM_BASE);
        cpu.registers[13] = 0x0300_0100;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.registers[13], 0x0300_0140);
        assert_eq!(cpu.registers[15], ROM_BASE + 0x40);
        assert!(cpu.is_thumb());
    }

    #[test]
    fn thumb_rejects_exactly_the_armv4t_undefined_encodings() {
        // Undefined on ARMv4T: unused B1-B3/B6-BB/BE-BF miscellaneous space, B<cond=AL> (DExx),
        // and the ARMv5 BLX suffix (E800-EFFF). DFxx is SWI and reports BIOS errors separately.
        let is_undefined = |opcode: u16| {
            matches!(opcode >> 8, 0xB1..=0xB3 | 0xB6..=0xBB | 0xBE | 0xBF | 0xDE)
                || opcode & 0xF800 == 0xE800
        };
        let mut memory = MemoryMap::new();
        for opcode in 0..=u16::MAX {
            memory.load_cartridge(&program(&[opcode, 0, 0]));
            let mut cpu = ThumbCpu::new(ROM_BASE);
            cpu.registers[13] = 0x0300_0100;

            let rejected = matches!(
                cpu.step(&mut memory),
                Err(CpuError::UnsupportedInstruction { .. })
            );

            assert_eq!(rejected, is_undefined(opcode), "opcode {opcode:04X}");
        }
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
}
