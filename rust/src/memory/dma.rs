use super::*;

/// Internal DMA state latched when a channel is enabled.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DmaChannel {
    pub(super) source: u32,
    pub(super) destination: u32,
    pub(super) count: u32,
    pub(super) pending: bool,
}

impl DmaChannel {
    pub(super) fn source_mask(channel: usize) -> u32 {
        if channel == 0 {
            0x07FF_FFFF
        } else {
            0x0FFF_FFFF
        }
    }

    pub(super) fn destination_mask(channel: usize) -> u32 {
        if channel == 3 {
            0x0FFF_FFFF
        } else {
            0x07FF_FFFF
        }
    }

    pub(super) fn control_mask(channel: usize) -> u16 {
        if channel == 3 { 0xFFE0 } else { 0xF7E0 }
    }

    /// A zero count means the maximum: 0x4000 units for DMA0-2 and 0x10000 for DMA3.
    pub(super) fn unit_count(channel: usize, encoded: u16) -> u32 {
        let (mask, maximum) = if channel == 3 {
            (0xFFFF, 0x1_0000)
        } else {
            (0x3FFF, 0x4000)
        };
        match u32::from(encoded) & mask {
            0 => maximum,
            count => count,
        }
    }
}

impl_state!(DmaChannel {
    source,
    destination,
    count,
    pending
});

impl MemoryMap {
    /// Starts DMA1 or DMA2 when it is set up for special timing towards this FIFO.
    pub(super) fn request_sound_dma(&mut self, fifo: usize) {
        let address = SOUND_FIFO_A_ADDR + fifo as u32 * 4;
        for channel in 1..=2 {
            let control = self.dma_control(channel);
            if control & DMA_ENABLE != 0
                && (control >> 12) & 3 == DMA_TIMING_SPECIAL
                && self.dma[channel].destination == address
            {
                self.dma[channel].pending = true;
            }
        }
    }

    pub(super) fn dma_control(&self, channel: usize) -> u16 {
        let offset = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE + 10;
        u16::from_le_bytes([self.io[offset], self.io[offset + 1]])
    }

    pub(super) fn dma_register_word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.io[offset..offset + 4].try_into().unwrap())
    }

    pub(super) fn dma_count_register(&self, channel: usize) -> u16 {
        let offset = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE + 8;
        u16::from_le_bytes([self.io[offset], self.io[offset + 1]])
    }

    pub(super) fn write_dma_control(&mut self, offset: usize, value: u8) {
        let channel = (offset - DMA_REGISTER_BASE) / DMA_REGISTER_STRIDE;
        let was_enabled = self.dma_control(channel) & DMA_ENABLE != 0;
        let byte_mask = (DmaChannel::control_mask(channel) >> ((offset & 1) * 8)) as u8;
        self.io[offset] = value & byte_mask;

        let control = self.dma_control(channel);
        if control & DMA_ENABLE == 0 {
            self.dma[channel].pending = false;
            return;
        }
        if !was_enabled {
            let base = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE;
            self.dma[channel] = DmaChannel {
                source: self.dma_register_word(base) & DmaChannel::source_mask(channel),
                destination: self.dma_register_word(base + 4)
                    & DmaChannel::destination_mask(channel),
                count: DmaChannel::unit_count(channel, self.dma_count_register(channel)),
                pending: (control >> 12) & 3 == DMA_TIMING_IMMEDIATE,
            };
        }
    }

    pub(super) fn trigger_dma(&mut self, timing: u16) {
        for channel in 0..4 {
            let control = self.dma_control(channel);
            if control & DMA_ENABLE != 0 && (control >> 12) & 3 == timing {
                self.dma[channel].pending = true;
            }
        }
    }

    /// Runs the highest-priority pending DMA transfer to completion and returns the cycles
    /// the CPU is stalled, or 0 when no transfer is pending. Sound FIFO and video capture
    /// timing (start timing 3) are not modeled and never trigger.
    pub fn run_pending_dma(&mut self) -> u32 {
        let Some(channel) = (0..4).find(|&channel| self.dma[channel].pending) else {
            return 0;
        };
        let control = self.dma_control(channel);
        let sound_fifo = matches!(channel, 1 | 2) && (control >> 12) & 3 == DMA_TIMING_SPECIAL;
        let unit: u32 = if control & DMA_WORD != 0 || sound_fifo {
            4
        } else {
            2
        };
        let step = |address_control: u16| match address_control {
            0 | 3 => unit as i32,
            1 => -(unit as i32),
            _ => 0,
        };
        let destination_control = if sound_fifo { 2 } else { (control >> 5) & 3 };
        let mut source_control = (control >> 7) & 3;
        // GamePak sources always increment.
        if (0x0800_0000..0x0E00_0000).contains(&self.dma[channel].source) {
            source_control = 0;
        }
        let source_step = step(source_control) as u32;
        let destination_step = step(destination_control) as u32;

        let DmaChannel {
            mut source,
            mut destination,
            count,
            ..
        } = self.dma[channel];
        let count = if sound_fifo { SOUND_DMA_WORDS } else { count };
        if self.in_eeprom_window(destination) {
            self.backup.observe_eeprom_dma_write(count);
        }
        let reads_eeprom = unit == 2 && self.in_eeprom_window(source);
        for _ in 0..count {
            if reads_eeprom {
                let value = self.backup.read_eeprom_bit();
                self.write_u16(destination & !1, value);
            } else if unit == 4 {
                let value = self.read_u32(source & !3);
                self.write_u32(destination & !3, value);
            } else {
                let value = self.read_u16(source & !1);
                self.write_u16(destination & !1, value);
            }
            source = source.wrapping_add(source_step);
            destination = destination.wrapping_add(destination_step);
        }

        let state = &mut self.dma[channel];
        state.source = source;
        state.destination = destination;
        state.pending = false;
        let timing = (control >> 12) & 3;
        if control & DMA_REPEAT != 0 && timing != DMA_TIMING_IMMEDIATE {
            self.dma[channel].count =
                DmaChannel::unit_count(channel, self.dma_count_register(channel));
            if destination_control == 3 {
                let base = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE;
                self.dma[channel].destination =
                    self.dma_register_word(base + 4) & DmaChannel::destination_mask(channel);
            }
        } else {
            let offset = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE + 11;
            self.io[offset] &= !((DMA_ENABLE >> 8) as u8);
        }
        if control & DMA_IRQ_ENABLE != 0 {
            self.request_irq(IRQ_DMA0 << channel);
        }
        DMA_SETUP_CYCLES + count * 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configure_dma(
        memory: &mut MemoryMap,
        channel: u32,
        source: u32,
        destination: u32,
        count: u16,
        control: u16,
    ) {
        let base = DMA0_ADDR + channel * DMA_REGISTER_STRIDE as u32;
        memory.write_u32(base, source);
        memory.write_u32(base + 4, destination);
        memory.write_u16(base + 8, count);
        memory.write_u16(base + 10, control);
    }

    #[test]
    fn immediate_dma_copies_words_clears_enable_and_requests_irq() {
        let mut memory = MemoryMap::new();
        for word in 0..4 {
            memory.write_u32(0x0200_0000 + word * 4, 0xA0B0_C0D0 + word);
        }
        configure_dma(
            &mut memory,
            3,
            0x0200_0000,
            0x0300_0000,
            4,
            DMA_ENABLE | DMA_IRQ_ENABLE | DMA_WORD,
        );
        assert_ne!(memory.read_u16(DMA0_ADDR + 3 * 12 + 10) & DMA_ENABLE, 0);

        let cycles = memory.run_pending_dma();

        for word in 0..4 {
            assert_eq!(memory.read_u32(0x0300_0000 + word * 4), 0xA0B0_C0D0 + word);
        }
        assert_eq!(cycles, DMA_SETUP_CYCLES + 8);
        assert_eq!(memory.read_u16(DMA0_ADDR + 3 * 12 + 10) & DMA_ENABLE, 0);
        assert_eq!(memory.read_u16(IF_ADDR), IRQ_DMA0 << 3);
        assert_eq!(memory.run_pending_dma(), 0);
    }

    #[test]
    fn fixed_source_halfword_dma_fills_and_decrementing_destination_moves_down() {
        let mut memory = MemoryMap::new();
        memory.write_u16(0x0200_0000, 0x1234);
        configure_dma(
            &mut memory,
            0,
            0x0200_0000,
            0x0300_0006,
            3,
            DMA_ENABLE | (2 << 7) | (1 << 5),
        );

        memory.run_pending_dma();

        assert_eq!(memory.read_u16(0x0300_0006), 0x1234);
        assert_eq!(memory.read_u16(0x0300_0004), 0x1234);
        assert_eq!(memory.read_u16(0x0300_0002), 0x1234);
        assert_eq!(memory.read_u16(0x0300_0000), 0);
    }

    #[test]
    fn repeating_hblank_dma_runs_on_visible_lines_and_reloads_its_destination() {
        let mut memory = MemoryMap::new();
        memory.write_u16(0x0200_0000, 0x0001);
        memory.write_u16(0x0200_0002, 0x0002);
        configure_dma(
            &mut memory,
            0,
            0x0200_0000,
            0x0300_0000,
            1,
            DMA_ENABLE | DMA_REPEAT | (DMA_TIMING_HBLANK << 12) | (3 << 5),
        );
        assert_eq!(memory.run_pending_dma(), 0, "HBlank DMA waits for HBlank");

        memory.advance_video_cycles(HBLANK_START_CYCLE);
        assert_ne!(memory.run_pending_dma(), 0);
        assert_eq!(memory.read_u16(0x0300_0000), 0x0001);
        assert_ne!(
            memory.read_u16(DMA0_ADDR + 10) & DMA_ENABLE,
            0,
            "repeat keeps enable"
        );

        memory.advance_video_cycles(CYCLES_PER_SCANLINE);
        memory.run_pending_dma();
        assert_eq!(memory.read_u16(0x0300_0000), 0x0002, "destination reloads");
        assert_eq!(memory.read_u16(0x0300_0002), 0);

        // Move into VBlank; HBlanks there must not trigger the transfer.
        memory.advance_video_cycles(
            (VISIBLE_SCANLINES - 1) * CYCLES_PER_SCANLINE - HBLANK_START_CYCLE,
        );
        memory.run_pending_dma();
        memory.advance_video_cycles(HBLANK_START_CYCLE + 1);
        assert_eq!(memory.run_pending_dma(), 0);
    }

    #[test]
    fn eeprom_is_accessed_through_dma3_in_the_top_rom_window() {
        let mut rom = vec![0; 0x200];
        rom.extend_from_slice(b"EEPROM_V122");
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);
        assert_eq!(
            memory.read_u16(0x0C00_0200),
            u16::from_le_bytes(*b"EE"),
            "the ROM mirror below 0x0D000000 is unchanged"
        );

        // Write request for 6-bit block 1: 10, 000001, 64 bits of 0xA5.., stop = 73 units.
        let request: Vec<u16> = [1, 0, 0, 0, 0, 0, 0, 1]
            .into_iter()
            .chain((0..64).map(|bit| u16::from(0xA5u8 >> (7 - bit % 8) & 1)))
            .chain([0])
            .collect();
        for (index, bit) in request.iter().enumerate() {
            memory.write_u16(0x0200_0000 + index as u32 * 2, *bit);
        }
        configure_dma(&mut memory, 3, 0x0200_0000, 0x0D00_0000, 73, DMA_ENABLE);
        memory.run_pending_dma();
        assert_eq!(&memory.backup.data()[8..16], &[0xA5; 8]);

        // Read request (9 units), then a 68-unit DMA read back into EWRAM.
        for (index, bit) in [1u16, 1, 0, 0, 0, 0, 0, 1, 0].iter().enumerate() {
            memory.write_u16(0x0200_0000 + index as u32 * 2, *bit);
        }
        configure_dma(&mut memory, 3, 0x0200_0000, 0x0D00_0000, 9, DMA_ENABLE);
        memory.run_pending_dma();
        configure_dma(&mut memory, 3, 0x0D00_0000, 0x0300_0000, 68, DMA_ENABLE);
        memory.run_pending_dma();
        let bits: Vec<u16> = (0..68)
            .map(|i| memory.read_u16(0x0300_0000 + i * 2))
            .collect();
        assert_eq!(&bits[4..12], &[1, 0, 1, 0, 0, 1, 0, 1]);
        assert_eq!(memory.read_u16(0x0D00_0000), 1, "ready when idle");
    }
}
