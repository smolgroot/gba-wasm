use super::*;

impl ThumbCpu {
    pub(super) fn bios_instruction_cycles(&self, service: u32) -> u32 {
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

    pub(super) fn register_ram_reset(&self, memory: &mut MemoryMap, flags: u32) {
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

    /// SoftReset (GBATEK "SWI 00h"; mGBA's hle-bios.s `SoftReset`). The entry flag at
    /// `0x03007FFA` is read before the top `0x200` bytes of IWRAM (stacks, IRQ handler
    /// pointer, BIOS flags) are cleared. The CPU ends in System mode, ARM state, with the
    /// boot stacks, zeroed r0-r12, `LR_svc`/`SPSR_svc`/`LR_irq`/`SPSR_irq`, and `LR` set to
    /// the entry. Flags are left clear; GBATEK does not specify them.
    pub(super) fn soft_reset(&mut self, memory: &mut MemoryMap) {
        let enters_ewram = memory.iwram[IWRAM_SIZE - 6] != 0;
        memory.iwram[IWRAM_SIZE - 0x200..].fill(0);

        self.switch_mode(CPSR_SYSTEM_MODE);
        self.cpsr = CPSR_SYSTEM_MODE;
        self.registers[..13].fill(0);
        self.registers[13] = HLE_SYSTEM_STACK;
        self.irq_bank = [HLE_IRQ_STACK, 0];
        self.svc_bank = [HLE_SVC_STACK, 0];
        for mode in [CPSR_IRQ_MODE, CPSR_SVC_MODE] {
            if let Some(index) = Self::spsr_index(mode) {
                self.spsr_bank[index] = 0;
            }
        }

        let entry = if enters_ewram { EWRAM_BASE } else { ROM_BASE };
        self.registers[14] = entry;
        self.registers[15] = entry;
        self.halt = None;
        self.intr_wait_resumed = false;
        self.idle_loop = None;
    }

    pub(super) fn cpu_set(&self, memory: &mut MemoryMap) {
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

    pub(super) fn cpu_fast_set(&self, memory: &mut MemoryMap) {
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
    pub(super) fn bg_affine_set(&self, memory: &mut MemoryMap) {
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
    pub(super) fn obj_affine_set(&self, memory: &mut MemoryMap) {
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

    pub(super) fn execute_bios_call(
        &mut self,
        memory: &mut MemoryMap,
        address: u32,
        state: ExecutionState,
        service: u8,
    ) -> Result<(), CpuError> {
        // The hardware BIOS returns from every service through the same opcode.
        memory.set_bios_latch(BIOS_LATCH_AFTER_SWI);
        match service {
            0x00 => {
                self.soft_reset(memory);
                Ok(())
            }
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
            0x1F => {
                let frequency = memory.read_u32(self.registers[0].wrapping_add(4));
                self.registers[0] =
                    midi_key_to_frequency(frequency, self.registers[1], self.registers[2]);
                Ok(())
            }
            _ => Err(CpuError::UnsupportedBiosCall {
                address,
                state,
                service,
            }),
        }
    }
}

/// Writes decompressed data in the unit size of the BIOS variant: bytes for the WRAM
/// variants, halfwords for VRAM, and words for Huffman. A trailing partial unit is dropped.
pub(super) fn write_decompressed(
    memory: &mut MemoryMap,
    destination: u32,
    data: &[u8],
    unit_size: usize,
) {
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
pub(super) fn compression_header(memory: &MemoryMap, source: u32) -> Option<usize> {
    if source & 0x0E00_0000 == 0 {
        return Some(0);
    }
    Some((memory.read_u32(source & !3) >> 8) as usize)
}

/// Decodes a BIOS Huffman stream (header type 2) with 4- or 8-bit data units.
pub(super) fn huffman_decompress(memory: &MemoryMap, source: u32) -> Option<Vec<u8>> {
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
pub(super) fn run_length_decompress(memory: &MemoryMap, source: u32) -> Option<Vec<u8>> {
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
pub(super) fn lz77_decompress(memory: &MemoryMap, source: u32) -> Option<Vec<u8>> {
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
pub(crate) fn affine_matrix(scale_x: i32, scale_y: i32, angle: u16) -> [i32; 4] {
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

/// BIOS `MidiKey2Freq`: `frequency / 2^((180 - key - fine / 256) / 12)`, where key 180
/// plays the sample at its recorded rate. Evaluated in single precision; results may
/// differ from the BIOS lookup tables in the lowest bits.
pub(super) fn midi_key_to_frequency(frequency: u32, key: u32, fine_pitch: u32) -> u32 {
    let semitones = 180.0 - key as f32 - fine_pitch as f32 / 256.0;
    (frequency as f32 / (semitones / 12.0).exp2()) as u32
}

/// BIOS `ArcTan` polynomial for a 1.14 fixed-point tangent. Returns the angle
/// (0x4000 = 90 degrees) and the intermediate values the BIOS leaves in r1 and r3.
pub(super) fn arctan(tangent: i32) -> (i32, i32, i32) {
    let a = -(tangent.wrapping_mul(tangent) >> 14);
    let mut b = ((0xA9 * a) >> 14) + 0x390;
    for coefficient in [0x91C, 0xFB6, 0x16AA, 0x2081, 0x3651, 0xA2F9] {
        b = (b.wrapping_mul(a) >> 14) + coefficient;
    }
    (tangent.wrapping_mul(b) >> 16, a, b)
}

/// BIOS `ArcTan2`: the angle of (x, y) as 0x0000-0xFFFF for a full turn.
pub(super) fn arctan2(x: i32, y: i32) -> u16 {
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

pub(crate) fn integer_square_root(value: u32) -> u32 {
    let mut root = (value as f64).sqrt() as u32;
    while u64::from(root) * u64::from(root) > u64::from(value) {
        root -= 1;
    }
    while u64::from(root + 1) * u64::from(root + 1) <= u64::from(value) {
        root += 1;
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::test_util::*;

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
    fn unsupported_thumb_swi_is_reported_explicitly() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF10])); // BitUnPack
        let mut cpu = ThumbCpu::new(ROM_BASE);

        assert_eq!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedBiosCall {
                address: ROM_BASE,
                state: ExecutionState::Thumb,
                service: 0x10,
            })
        );
        assert_eq!(cpu.registers[15], ROM_BASE);
    }

    #[test]
    fn thumb_swi_midi_key_to_freq_scales_the_wave_data_frequency() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0xDF1F, 0xDF1F, 0xDF1F]));
        memory.write_u32(0x0200_0004, 44_100 << 10); // WaveData.freq
        let mut cpu = ThumbCpu::new(ROM_BASE);

        // Key 180 plays the sample at its recorded rate.
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 180;
        cpu.registers[2] = 0;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 44_100 << 10);

        // One octave lower halves it.
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 168;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 44_100 << 9);

        // A full semitone of fine pitch matches the next key.
        cpu.registers[0] = 0x0200_0000;
        cpu.registers[1] = 155;
        cpu.registers[2] = 256;
        cpu.step(&mut memory).unwrap();
        assert_eq!(cpu.registers[0], 44_100 << 8);
        assert_eq!(cpu.registers[1], 155);
        assert_eq!(cpu.registers[15], ROM_BASE + 6);
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
    fn soft_reset_restarts_at_rom_in_system_mode_with_boot_stacks() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&program(&[0x0000, 0xDF00])); // MOVS r0, r0; SWI 0x00
        memory.iwram.fill(0xA5);
        memory.iwram[0x7FFA] = 0; // entry flag: ROM
        let mut cpu = ThumbCpu::boot_hle();
        cpu.cpsr |= CPSR_THUMB | FLAG_N | FLAG_Z | FLAG_C | FLAG_V;
        cpu.registers[..15].fill(0xDEAD_BEEF);
        cpu.registers[15] = ROM_BASE + 2;
        cpu.svc_bank = [1, 2];
        cpu.irq_bank = [3, 4];
        cpu.spsr_bank = [5; 5];

        cpu.step(&mut memory).unwrap();

        assert_eq!(
            cpu.cpsr, CPSR_SYSTEM_MODE,
            "ARM state, System mode, IRQs enabled"
        );
        assert_eq!(cpu.registers[..13], [0; 13]);
        assert_eq!(cpu.registers[13], 0x0300_7F00);
        assert_eq!(cpu.registers[14], ROM_BASE);
        assert_eq!(cpu.registers[15], ROM_BASE);
        assert_eq!(cpu.irq_bank, [0x0300_7FA0, 0]);
        assert_eq!(cpu.svc_bank, [0x0300_7FE0, 0]);
        assert_eq!(
            cpu.spsr_bank,
            [5, 0, 0, 5, 5],
            "only the IRQ and SVC SPSRs are cleared"
        );
        assert!(memory.iwram[0x7E00..].iter().all(|byte| *byte == 0));
        assert!(memory.iwram[..0x7E00].iter().all(|byte| *byte == 0xA5));
    }

    #[test]
    fn soft_reset_from_irq_mode_enters_ewram_when_flag_is_set() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&arm_word(0xEF00_0000)); // SWI 0x00
        memory.iwram[0x7FFA] = 1; // entry flag: EWRAM
        let mut cpu = ThumbCpu::boot_hle();
        cpu.enter_exception(CPSR_IRQ_MODE, ROM_BASE, 0x1234);
        cpu.registers[13] = 0x0300_7000;

        cpu.step(&mut memory).unwrap();

        assert_eq!(cpu.cpsr, CPSR_SYSTEM_MODE);
        assert_eq!(cpu.registers[13], 0x0300_7F00);
        assert_eq!(cpu.registers[14], 0x0200_0000);
        assert_eq!(cpu.registers[15], 0x0200_0000);
        assert_eq!(
            cpu.irq_bank,
            [0x0300_7FA0, 0],
            "the IRQ bank is reset, not saved"
        );
        assert_eq!(cpu.spsr_bank[1], 0);
        assert_eq!(memory.iwram[0x7FFA], 0, "the flag itself is cleared");
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
