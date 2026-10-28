pub const EWRAM_SIZE: usize = 256 * 1024;
pub const IWRAM_SIZE: usize = 32 * 1024;
pub const VRAM_SIZE: usize = 96 * 1024;
pub const OAM_SIZE: usize = 1 * 1024;
pub const PALETTE_SIZE: usize = 1 * 1024;
pub const IO_BASE: u32 = 0x0400_0000;
pub const IO_SIZE: usize = 1 * 1024;
pub const DISPCNT_ADDR: u32 = IO_BASE;
pub const DISPSTAT_ADDR: u32 = IO_BASE + 0x004;
pub const VCOUNT_ADDR: u32 = IO_BASE + 0x006;
pub const CYCLES_PER_SCANLINE: u32 = 1232;
pub const SCANLINES_PER_FRAME: u32 = 228;
pub const VISIBLE_SCANLINES: u32 = 160;
pub const CYCLES_PER_FRAME: u32 = CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME;
pub const KEYINPUT_ADDR: u32 = IO_BASE + 0x130;
pub const KEYCNT_ADDR: u32 = IO_BASE + 0x132;
pub const TIMER0_ADDR: u32 = IO_BASE + 0x100;
pub const IE_ADDR: u32 = IO_BASE + 0x200;
pub const IF_ADDR: u32 = IO_BASE + 0x202;
pub const WAITCNT_ADDR: u32 = IO_BASE + 0x204;
pub const IME_ADDR: u32 = IO_BASE + 0x208;

const DISPSTAT_IRQ_ENABLE_MASK: u8 = 0b0011_1000;
const DISPCNT_LOW_WRITABLE_MASK: u8 = 0xF7;
const DISPSTAT_VBLANK: u8 = 1 << 0;
const DISPSTAT_HBLANK: u8 = 1 << 1;
const DISPSTAT_VCOUNT_MATCH: u8 = 1 << 2;
const HBLANK_START_CYCLE: u32 = 1006;
const IRQ_TIMER0: u16 = 1 << 3;
const IRQ_VBLANK: u16 = 1 << 0;
const IRQ_HBLANK: u16 = 1 << 1;
const IRQ_VCOUNT: u16 = 1 << 2;
const IRQ_DMA0: u16 = 1 << 8;
const IRQ_KEYPAD: u16 = 1 << 12;
pub const DMA0_ADDR: u32 = IO_BASE + 0x0B0;
const DMA_REGISTER_BASE: usize = 0x0B0;
const DMA_REGISTER_STRIDE: usize = 12;
const DMA_REPEAT: u16 = 1 << 9;
const DMA_WORD: u16 = 1 << 10;
const DMA_IRQ_ENABLE: u16 = 1 << 14;
const DMA_ENABLE: u16 = 1 << 15;
const DMA_TIMING_IMMEDIATE: u16 = 0;
const DMA_TIMING_VBLANK: u16 = 1;
const DMA_TIMING_HBLANK: u16 = 2;
/// Fixed setup cost per transfer; each unit adds a read and a write cycle.
const DMA_SETUP_CYCLES: u32 = 2;
const KEYINPUT_MASK: u16 = 0x03FF;
const DISPLAY_MODE_MASK: u8 = 0x07;
const TILE_MODE_BG_VRAM_END: usize = 0x1_0000;
const BITMAP_MODE_BG_VRAM_END: usize = 0x1_4000;

#[derive(Debug, Clone, Copy, Default)]
struct Timer {
    reload: u16,
    counter: u16,
    control: u8,
    prescaler_phase: u64,
}

impl Timer {
    const ENABLE: u8 = 1 << 7;
    const CASCADE: u8 = 1 << 2;
    const IRQ_ENABLE: u8 = 1 << 6;

    fn advance(&mut self, ticks: u64) -> u64 {
        if ticks == 0 {
            return 0;
        }

        let ticks_to_overflow = u64::from(u16::MAX) + 1 - u64::from(self.counter);
        if ticks < ticks_to_overflow {
            self.counter = self.counter.wrapping_add(ticks as u16);
            return 0;
        }

        let ticks_after_overflow = ticks - ticks_to_overflow;
        let reload_period = u64::from(u16::MAX) + 1 - u64::from(self.reload);
        let overflows = 1 + ticks_after_overflow / reload_period;
        let remainder = ticks_after_overflow % reload_period;
        self.counter = self.reload.wrapping_add(remainder as u16);
        overflows
    }
}

/// Internal DMA state latched when a channel is enabled.
#[derive(Debug, Clone, Copy, Default)]
struct DmaChannel {
    source: u32,
    destination: u32,
    count: u32,
    pending: bool,
}

impl DmaChannel {
    fn source_mask(channel: usize) -> u32 {
        if channel == 0 {
            0x07FF_FFFF
        } else {
            0x0FFF_FFFF
        }
    }

    fn destination_mask(channel: usize) -> u32 {
        if channel == 3 {
            0x0FFF_FFFF
        } else {
            0x07FF_FFFF
        }
    }

    fn control_mask(channel: usize) -> u16 {
        if channel == 3 { 0xFFE0 } else { 0xF7E0 }
    }

    /// A zero count means the maximum: 0x4000 units for DMA0-2 and 0x10000 for DMA3.
    fn unit_count(channel: usize, encoded: u16) -> u32 {
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

/// True when an HBlank starts on a visible scanline in `(start, start + elapsed]`.
fn visible_hblank_occurred(start: u32, elapsed: u64) -> bool {
    if elapsed >= u64::from(CYCLES_PER_FRAME) {
        return true;
    }
    let scanline_cycles = u64::from(CYCLES_PER_SCANLINE);
    let start = u64::from(start);
    let end = start + elapsed;
    let mut event = start / scanline_cycles * scanline_cycles + u64::from(HBLANK_START_CYCLE);
    if event <= start {
        event += scanline_cycles;
    }
    while event <= end {
        let scanline = (event / scanline_cycles) % u64::from(SCANLINES_PER_FRAME);
        if scanline < u64::from(VISIBLE_SCANLINES) {
            return true;
        }
        event += scanline_cycles;
    }
    false
}

fn vram_index(addr: u32) -> usize {
    let offset = ((addr - 0x0600_0000) as usize) & 0x1_FFFF;
    if offset >= 0x1_8000 {
        offset - 0x8000
    } else {
        offset
    }
}

/// Read mask for stored I/O registers that read back what was written. Registers not listed
/// here and not modeled elsewhere are write-only or unimplemented and read as zero.
fn stored_io_read_mask(offset: usize) -> u8 {
    match offset {
        0x002..=0x003 => 0xFF,                     // green swap
        0x008..=0x00B => [0xFF, 0xDF][offset & 1], // BG0CNT/BG1CNT (no overflow bit)
        0x00C..=0x00F => 0xFF,                     // BG2CNT/BG3CNT
        0x048..=0x04B => 0x3F,                     // WININ/WINOUT
        0x050 => 0xFF,                             // BLDCNT
        0x051 => 0x3F,
        0x052..=0x053 => 0x1F, // BLDALPHA
        0x0BA..=0x0BB | 0x0C6..=0x0C7 | 0x0D2..=0x0D3 | 0x0DE..=0x0DF => 0xFF, // DMAxCNT_H
        0x300 => 0x01,         // POSTFLG
        _ => 0,
    }
}

fn periodic_event_count(start: u32, elapsed: u64, event: u32, period: u32) -> u64 {
    if elapsed == 0 || period == 0 {
        return 0;
    }

    let phase = start % period;
    let event_phase = event % period;
    let distance = if event_phase > phase {
        event_phase - phase
    } else {
        period - (phase - event_phase)
    };
    let distance = if distance == 0 { period } else { distance };

    if elapsed < u64::from(distance) {
        0
    } else {
        1 + (elapsed - u64::from(distance)) / u64::from(period)
    }
}

#[derive(Debug, Clone)]
pub struct MemoryMap {
    pub bios: Vec<u8>,
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub palette: Vec<u8>,
    pub io: Vec<u8>,
    pub cartridge: Vec<u8>,
    video_cycles: u32,
    vblanks_started: u64,
    pressed_keys: u16,
    timers: [Timer; 4],
    dma: [DmaChannel; 4],
}

impl MemoryMap {
    pub fn new() -> Self {
        Self {
            bios: crate::bios::hle_image(),
            ewram: vec![0; EWRAM_SIZE],
            iwram: vec![0; IWRAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
            palette: vec![0; PALETTE_SIZE],
            io: vec![0; IO_SIZE],
            cartridge: Vec::new(),
            video_cycles: 0,
            vblanks_started: 0,
            pressed_keys: 0,
            timers: [Timer::default(); 4],
            dma: [DmaChannel::default(); 4],
        }
    }

    pub fn load_cartridge(&mut self, data: &[u8]) {
        self.cartridge.clear();
        self.cartridge.extend_from_slice(data);
    }

    pub fn advance_video_cycles(&mut self, cycles: u32) {
        self.advance_timers(u64::from(cycles));
        self.advance_display_interrupts(self.video_cycles, u64::from(cycles));
        let vblanks = periodic_event_count(
            self.video_cycles,
            u64::from(cycles),
            VISIBLE_SCANLINES * CYCLES_PER_SCANLINE,
            CYCLES_PER_FRAME,
        );
        if vblanks != 0 {
            self.trigger_dma(DMA_TIMING_VBLANK);
        }
        if visible_hblank_occurred(self.video_cycles, u64::from(cycles)) {
            self.trigger_dma(DMA_TIMING_HBLANK);
        }
        self.vblanks_started += vblanks;
        self.video_cycles = (self.video_cycles + cycles % CYCLES_PER_FRAME) % CYCLES_PER_FRAME;
    }

    pub fn reset_video_clock(&mut self) {
        self.video_cycles = 0;
        self.vblanks_started = 0;
    }

    /// Number of times the display has entered VBlank (scanline 160) since the clock reset.
    pub fn vblanks_started(&self) -> u64 {
        self.vblanks_started
    }

    /// Cycles until the next display or timer event that can change `IF`, so a halted CPU
    /// can skip idle cycles without moving any interrupt request.
    pub fn cycles_until_next_event(&self) -> u32 {
        let line_cycle = self.video_cycles % CYCLES_PER_SCANLINE;
        let mut cycles = if line_cycle < HBLANK_START_CYCLE {
            HBLANK_START_CYCLE - line_cycle
        } else {
            CYCLES_PER_SCANLINE - line_cycle
        };

        let prescalers = [1u64, 64, 256, 1024];
        for (index, timer) in self.timers.iter().enumerate() {
            let counts_cycles = index == 0 || timer.control & Timer::CASCADE == 0;
            if timer.control & Timer::ENABLE == 0 || !counts_cycles {
                continue;
            }
            let divisor = prescalers[usize::from(timer.control & 0x3)];
            let ticks_to_overflow = u64::from(u16::MAX) + 1 - u64::from(timer.counter);
            let until_overflow = ticks_to_overflow * divisor - timer.prescaler_phase;
            cycles = cycles.min(until_overflow.min(u64::from(u32::MAX)) as u32);
        }
        cycles.max(1)
    }

    pub fn reset_io_registers(&mut self) {
        self.io.fill(0);
        self.timers = [Timer::default(); 4];
        self.dma = [DmaChannel::default(); 4];
    }

    pub fn set_pressed_keys(&mut self, pressed: u16) {
        let old_pressed = self.pressed_keys;
        self.pressed_keys = pressed & KEYINPUT_MASK;
        if !self.key_condition_matches(old_pressed) && self.key_condition_matches(self.pressed_keys)
        {
            self.request_keypad_interrupt();
        }
    }

    /// True when the CPU should take an IRQ exception (IME set and an enabled request pending).
    pub fn irq_line_asserted(&self) -> bool {
        self.io[0x208] & 1 != 0 && self.halt_wake_requested()
    }

    /// True when `IE & IF` is non-zero; this wakes a halted CPU regardless of `IME`.
    pub fn halt_wake_requested(&self) -> bool {
        let enabled = u16::from(self.io[0x200]) | (u16::from(self.io[0x201]) << 8);
        let pending = u16::from(self.io[0x202]) | (u16::from(self.io[0x203]) << 8);
        enabled & pending & 0x3FFF != 0
    }

    fn request_irq(&mut self, irq: u16) {
        let pending = self.io[0x202] as u16 | (u16::from(self.io[0x203]) << 8);
        let pending = pending | irq;
        self.io[0x202] = pending as u8;
        self.io[0x203] = (pending >> 8) as u8;
    }

    fn advance_display_interrupts(&mut self, start: u32, elapsed: u64) {
        let enabled = self.io[0x004];
        if enabled & DISPSTAT_IRQ_ENABLE_MASK == 0 {
            return;
        }

        let frame = CYCLES_PER_FRAME;
        if enabled & (1 << 3) != 0
            && periodic_event_count(
                start,
                elapsed,
                VISIBLE_SCANLINES * CYCLES_PER_SCANLINE,
                frame,
            ) != 0
        {
            self.request_irq(IRQ_VBLANK);
        }
        if enabled & (1 << 4) != 0
            && periodic_event_count(start, elapsed, HBLANK_START_CYCLE, CYCLES_PER_SCANLINE) != 0
        {
            self.request_irq(IRQ_HBLANK);
        }
        let compare = u32::from(self.io[5]);
        if compare < SCANLINES_PER_FRAME
            && enabled & (1 << 5) != 0
            && periodic_event_count(start, elapsed, compare * CYCLES_PER_SCANLINE, frame) != 0
        {
            self.request_irq(IRQ_VCOUNT);
        }
    }

    fn key_condition_matches(&self, pressed: u16) -> bool {
        let control = u16::from(self.io[0x132]) | (u16::from(self.io[0x133]) << 8);
        let selected = control & KEYINPUT_MASK;
        if control & (1 << 14) == 0 || selected == 0 {
            return false;
        }
        if control & (1 << 15) != 0 {
            pressed & selected == selected
        } else {
            pressed & selected != 0
        }
    }

    fn request_keypad_interrupt(&mut self) {
        if self.key_condition_matches(self.pressed_keys) {
            self.request_irq(IRQ_KEYPAD);
        }
    }

    fn advance_timers(&mut self, cycles: u64) {
        let prescalers = [1u64, 64, 256, 1024];
        let mut prior_overflows = 0;
        for (index, timer) in self.timers.iter_mut().enumerate() {
            if timer.control & Timer::ENABLE == 0 {
                prior_overflows = 0;
                continue;
            }

            let ticks = if index > 0 && timer.control & Timer::CASCADE != 0 {
                prior_overflows
            } else {
                let divisor = prescalers[usize::from(timer.control & 0x3)];
                let accumulated = timer.prescaler_phase + cycles;
                timer.prescaler_phase = accumulated % divisor;
                accumulated / divisor
            };
            prior_overflows = timer.advance(ticks);

            if prior_overflows > 0 && timer.control & Timer::IRQ_ENABLE != 0 {
                let pending = self.io[0x202] as u16 | (u16::from(self.io[0x203]) << 8);
                let pending = pending | (IRQ_TIMER0 << index);
                self.io[0x202] = pending as u8;
                self.io[0x203] = (pending >> 8) as u8;
            }
        }
    }

    fn dma_control(&self, channel: usize) -> u16 {
        let offset = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE + 10;
        u16::from_le_bytes([self.io[offset], self.io[offset + 1]])
    }

    fn dma_register_word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.io[offset..offset + 4].try_into().unwrap())
    }

    fn dma_count_register(&self, channel: usize) -> u16 {
        let offset = DMA_REGISTER_BASE + channel * DMA_REGISTER_STRIDE + 8;
        u16::from_le_bytes([self.io[offset], self.io[offset + 1]])
    }

    fn write_dma_control(&mut self, offset: usize, value: u8) {
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

    fn trigger_dma(&mut self, timing: u16) {
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
        let unit: u32 = if control & DMA_WORD != 0 { 4 } else { 2 };
        let step = |address_control: u16| match address_control {
            0 | 3 => unit as i32,
            1 => -(unit as i32),
            _ => 0,
        };
        let destination_control = (control >> 5) & 3;
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
        for _ in 0..count {
            if unit == 4 {
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

    fn timer_io_byte(&self, offset: usize) -> u8 {
        let timer_index = (offset - 0x100) / 4;
        let timer_byte = (offset - 0x100) % 4;
        let timer = self.timers[timer_index];
        match timer_byte {
            0 => timer.counter as u8,
            1 => (timer.counter >> 8) as u8,
            2 => timer.control,
            3 => 0,
            _ => unreachable!(),
        }
    }

    fn read_io(&self, offset: usize) -> u8 {
        if (0x100..0x110).contains(&offset) {
            return self.timer_io_byte(offset);
        }

        let keyinput = !self.pressed_keys & KEYINPUT_MASK;
        match offset {
            0x000 => return self.io[offset] & DISPCNT_LOW_WRITABLE_MASK,
            0x001 => return self.io[offset],
            0x130 => return keyinput as u8,
            0x131 => return (keyinput >> 8) as u8,
            0x132 => return self.io[offset] & 0xFF,
            0x133 => return self.io[offset] & 0xC3,
            0x200 => return self.io[offset] & 0xFF,
            0x201 => return self.io[offset] & 0x3F,
            0x202 => return self.io[offset] & 0xFF,
            0x203 => return self.io[offset] & 0x3F,
            0x204 => return self.io[offset] & 0xFF,
            0x205 => return self.io[offset] & 0x7F,
            0x208 => return self.io[offset] & 1,
            0x209 => return 0,
            _ => {}
        }

        if !(0x004..=0x007).contains(&offset) {
            return self.io[offset] & stored_io_read_mask(offset);
        }

        let scanline = self.video_cycles / CYCLES_PER_SCANLINE;
        let scanline_cycle = self.video_cycles % CYCLES_PER_SCANLINE;
        let compare = self.io[5];
        let mut dispstat = self.io[4] & DISPSTAT_IRQ_ENABLE_MASK;

        if scanline >= VISIBLE_SCANLINES && scanline < SCANLINES_PER_FRAME - 1 {
            dispstat |= DISPSTAT_VBLANK;
        }
        if scanline_cycle >= HBLANK_START_CYCLE {
            dispstat |= DISPSTAT_HBLANK;
        }
        if scanline as u8 == compare {
            dispstat |= DISPSTAT_VCOUNT_MATCH;
        }

        match offset {
            0x004 => dispstat,
            0x005 => compare,
            0x006 => scanline as u8,
            0x007 => 0,
            _ => 0,
        }
    }

    fn write_io(&mut self, offset: usize, value: u8) {
        if (0x100..0x110).contains(&offset) {
            let timer_index = (offset - 0x100) / 4;
            let timer_byte = (offset - 0x100) % 4;
            let timer = &mut self.timers[timer_index];
            match timer_byte {
                0 => timer.reload = (timer.reload & 0xFF00) | u16::from(value),
                1 => timer.reload = (timer.reload & 0x00FF) | (u16::from(value) << 8),
                2 => {
                    let was_enabled = timer.control & Timer::ENABLE != 0;
                    timer.control = value & 0xC7;
                    if !was_enabled && timer.control & Timer::ENABLE != 0 {
                        timer.counter = timer.reload;
                        timer.prescaler_phase = 0;
                    } else if timer.control & Timer::ENABLE == 0 {
                        timer.prescaler_phase = 0;
                    }
                }
                3 => {}
                _ => unreachable!(),
            }
            return;
        }

        let key_condition_was_matched = self.key_condition_matches(self.pressed_keys);
        match offset {
            0x000 => self.io[offset] = value & DISPCNT_LOW_WRITABLE_MASK,
            0x001 => self.io[offset] = value,
            0x004 => {
                self.io[offset] = (self.io[offset] & !DISPSTAT_IRQ_ENABLE_MASK)
                    | (value & DISPSTAT_IRQ_ENABLE_MASK);
            }
            0x005 => self.io[offset] = value,
            0x006..=0x007 | 0x130..=0x131 | 0x209 => {}
            0x132 => self.io[offset] = value & 0xFF,
            0x133 => self.io[offset] = value & 0xC3,
            0x0BA..=0x0BB | 0x0C6..=0x0C7 | 0x0D2..=0x0D3 | 0x0DE..=0x0DF => {
                self.write_dma_control(offset, value)
            }
            0x200 => self.io[offset] = value,
            0x201 => self.io[offset] = value & 0x3F,
            0x202 => self.io[offset] &= !value,
            0x203 => self.io[offset] &= !(value & 0x3F),
            0x204 => self.io[offset] = value,
            0x205 => self.io[offset] = value & 0x7F,
            0x208 => self.io[offset] = value & 1,
            0x100..=0x10F => unreachable!(),
            // Kept for the devices that consume them; readability is decided in `read_io`.
            _ => self.io[offset] = value,
        }
        if matches!(offset, 0x132 | 0x133)
            && !key_condition_was_matched
            && self.key_condition_matches(self.pressed_keys)
        {
            self.request_keypad_interrupt();
        }
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        match addr {
            0x0000_0000..=0x0000_3FFF => self.bios[addr as usize],
            0x0200_0000..=0x02FF_FFFF => self.ewram[(addr as usize - 0x0200_0000) % EWRAM_SIZE],
            0x0300_0000..=0x03FF_FFFF => self.iwram[(addr as usize - 0x0300_0000) % IWRAM_SIZE],
            IO_BASE..=0x0400_03FF => self.read_io(addr as usize - IO_BASE as usize),
            0x0500_0000..=0x05FF_FFFF => self.palette[(addr as usize - 0x0500_0000) % PALETTE_SIZE],
            0x0600_0000..=0x06FF_FFFF => self.vram[vram_index(addr)],
            0x0700_0000..=0x07FF_FFFF => self.oam[(addr as usize - 0x0700_0000) % OAM_SIZE],
            0x0800_0000..=0x09FF_FFFF | 0x0A00_0000..=0x0BFF_FFFF | 0x0C00_0000..=0x0DFF_FFFF => {
                let offset = (addr as usize) & 0x01FF_FFFF;
                self.cartridge.get(offset).copied().unwrap_or(0xFF)
            }
            _ => 0,
        }
    }

    /// CPU byte store. Video memory has a 16-bit bus: palette and background VRAM byte
    /// stores write the byte to both halves of the halfword, while OBJ VRAM and OAM
    /// ignore byte stores.
    pub fn write_u8(&mut self, addr: u32, value: u8) {
        match addr {
            0x0500_0000..=0x05FF_FFFF => self.write_u16(addr, u16::from_le_bytes([value, value])),
            0x0600_0000..=0x06FF_FFFF => {
                if vram_index(addr) < self.background_vram_end() {
                    self.write_u16(addr, u16::from_le_bytes([value, value]));
                }
            }
            0x0700_0000..=0x07FF_FFFF => {}
            _ => self.store_u8(addr, value),
        }
    }

    fn background_vram_end(&self) -> usize {
        if self.io[0] & DISPLAY_MODE_MASK >= 3 {
            BITMAP_MODE_BG_VRAM_END
        } else {
            TILE_MODE_BG_VRAM_END
        }
    }

    /// Stores one byte of a halfword or word access.
    fn store_u8(&mut self, addr: u32, value: u8) {
        match addr {
            0x0200_0000..=0x02FF_FFFF => {
                let index = (addr as usize - 0x0200_0000) % EWRAM_SIZE;
                self.ewram[index] = value;
            }
            0x0300_0000..=0x03FF_FFFF => {
                let index = (addr as usize - 0x0300_0000) % IWRAM_SIZE;
                self.iwram[index] = value;
            }
            IO_BASE..=0x0400_03FF => {
                let offset = addr as usize - IO_BASE as usize;
                self.write_io(offset, value);
            }
            0x0500_0000..=0x05FF_FFFF => {
                let index = (addr as usize - 0x0500_0000) % PALETTE_SIZE;
                self.palette[index] = value;
            }
            0x0600_0000..=0x06FF_FFFF => {
                self.vram[vram_index(addr)] = value;
            }
            0x0700_0000..=0x07FF_FFFF => {
                let index = (addr as usize - 0x0700_0000) % OAM_SIZE;
                self.oam[index] = value;
            }
            0x0800_0000..=0x0DFF_FFFF => {}
            _ => {}
        }
    }

    pub fn read_u16(&self, addr: u32) -> u16 {
        let aligned_addr = addr & !1;
        let lo = self.read_u8(aligned_addr) as u16;
        let hi = self.read_u8(aligned_addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    pub fn read_u32(&self, addr: u32) -> u32 {
        let aligned_addr = addr & !3;
        u32::from(self.read_u16(aligned_addr))
            | (u32::from(self.read_u16(aligned_addr.wrapping_add(2))) << 16)
    }

    pub fn write_u16(&mut self, addr: u32, value: u16) {
        let aligned_addr = addr & !1;
        self.store_u8(aligned_addr, (value & 0xFF) as u8);
        self.store_u8(aligned_addr.wrapping_add(1), (value >> 8) as u8);
    }

    pub fn write_u32(&mut self, addr: u32, value: u32) {
        let aligned_addr = addr & !3;
        self.write_u16(aligned_addr, value as u16);
        self.write_u16(aligned_addr.wrapping_add(2), (value >> 16) as u16);
    }
}

impl Default for MemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vram_reads_and_writes_bytes_at_their_offsets() {
        let mut memory = MemoryMap::new();
        memory.write_u8(0x0600_1234, 0xAB);

        assert_eq!(memory.vram[0x1234], 0xAB);
        assert_eq!(memory.read_u8(0x0600_1234), 0xAB);
    }

    #[test]
    fn upper_vram_aperture_mirrors_the_last_32_kib() {
        let mut memory = MemoryMap::new();
        memory.write_u16(0x0601_8000, 0x5A);

        assert_eq!(memory.read_u8(0x0601_0000), 0x5A);
        assert_eq!(memory.read_u8(0x0601_8000), 0x5A);
    }

    #[test]
    fn cartridge_rom_is_mirrored_per_window_and_unloaded_addresses_read_ff() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0x12, 0x34, 0x56, 0x78]);
        memory.write_u8(0x0800_0000, 0xFF);

        assert_eq!(memory.read_u8(0x0800_0000), 0x12);
        assert_eq!(memory.read_u8(0x0A00_0000), 0x12);
        assert_eq!(memory.read_u8(0x0C00_0003), 0x78);
        assert_eq!(memory.read_u8(0x0800_0004), 0xFF);
        assert_eq!(memory.read_u8(0x09FF_FFFF), 0xFF);
    }

    #[test]
    fn io_registers_are_visible_only_in_the_documented_io_window() {
        let mut memory = MemoryMap::new();
        memory.write_u16(DISPCNT_ADDR, 0x0403);
        memory.write_u8(0x0400_0400, 0xFF);

        assert_eq!(memory.read_u16(DISPCNT_ADDR), 0x0403);
        assert_eq!(memory.read_u8(0x0400_03FF), 0);
        assert_eq!(memory.read_u8(0x0400_0400), 0);
    }

    #[test]
    fn unaligned_halfword_and_word_accesses_align_down() {
        let mut memory = MemoryMap::new();
        memory.write_u16(0x0200_0001, 0xABCD);
        assert_eq!(memory.read_u16(0x0200_0000), 0xABCD);
        assert_eq!(memory.read_u16(0x0200_0001), 0xABCD);

        memory.write_u32(0x0300_0003, 0x1234_5678);
        assert_eq!(memory.read_u32(0x0300_0000), 0x1234_5678);
        assert_eq!(memory.read_u32(0x0300_0003), 0x1234_5678);
    }

    #[test]
    fn work_ram_palette_and_oam_mirrors_stay_inside_their_regions() {
        let mut memory = MemoryMap::new();
        memory.write_u8(0x0200_0007, 0x11);
        memory.write_u8(0x0300_0007, 0x22);
        memory.write_u16(0x0500_0006, 0x3300);
        memory.write_u16(0x0700_0006, 0x4400);

        assert_eq!(memory.read_u8(0x0204_0007), 0x11);
        assert_eq!(memory.read_u8(0x0300_8007), 0x22);
        assert_eq!(memory.read_u8(0x0500_0407), 0x33);
        assert_eq!(memory.read_u8(0x0700_0407), 0x44);
        assert_eq!(memory.read_u8(0x0200_0008), 0);
        assert_eq!(memory.read_u8(0x0300_0008), 0);
        assert_eq!(memory.read_u8(0x0400_0007), 0);
    }

    #[test]
    fn video_byte_stores_follow_the_16_bit_bus_rules() {
        let mut memory = MemoryMap::new();
        memory.write_u8(0x0500_0021, 0x12);
        memory.write_u8(0x0600_0011, 0x34);
        memory.write_u8(0x0601_0010, 0x56);
        memory.write_u8(0x0700_0010, 0x78);

        assert_eq!(memory.read_u16(0x0500_0020), 0x1212);
        assert_eq!(memory.read_u16(0x0600_0010), 0x3434);
        assert_eq!(
            memory.read_u16(0x0601_0010),
            0,
            "OBJ VRAM ignores byte stores"
        );
        assert_eq!(memory.read_u16(0x0700_0010), 0, "OAM ignores byte stores");

        memory.write_u16(DISPCNT_ADDR, 3);
        memory.write_u8(0x0601_0010, 0x56);
        assert_eq!(
            memory.read_u16(0x0601_0010),
            0x5656,
            "bitmap modes extend BG VRAM"
        );
        memory.write_u8(0x0601_4010, 0x9A);
        assert_eq!(memory.read_u16(0x0601_4010), 0);
    }

    #[test]
    fn stored_io_registers_read_back_through_their_masks() {
        let mut memory = MemoryMap::new();
        memory.write_u16(IO_BASE + 0x008, 0xFFFF); // BG0CNT
        memory.write_u16(IO_BASE + 0x00C, 0xFFFF); // BG2CNT
        memory.write_u16(IO_BASE + 0x010, 0x0123); // BG0HOFS (write-only)
        memory.write_u16(IO_BASE + 0x050, 0xFFFF); // BLDCNT

        assert_eq!(memory.read_u16(IO_BASE + 0x008), 0xDFFF);
        assert_eq!(memory.read_u16(IO_BASE + 0x00C), 0xFFFF);
        assert_eq!(memory.read_u16(IO_BASE + 0x010), 0);
        assert_eq!(
            memory.io[0x010], 0x23,
            "write-only registers keep their value for devices"
        );
        assert_eq!(memory.read_u16(IO_BASE + 0x050), 0x3FFF);
    }

    #[test]
    fn display_control_masks_reserved_bit_and_unimplemented_io_is_inert() {
        let mut memory = MemoryMap::new();
        memory.write_u16(DISPCNT_ADDR, 0xFFFF);
        memory.write_u8(IO_BASE + 0x010, 0xA5);

        assert_eq!(memory.read_u16(DISPCNT_ADDR), 0xFFF7);
        assert_eq!(memory.read_u8(IO_BASE + 0x010), 0);
    }

    #[test]
    fn interrupt_flags_are_write_one_to_clear_and_timer_overflow_sets_request() {
        let mut memory = MemoryMap::new();
        memory.write_u16(IF_ADDR, 0x0008);
        assert_eq!(memory.read_u16(IF_ADDR), 0);

        memory.write_u16(TIMER0_ADDR, 0xFFFE);
        memory.write_u8(TIMER0_ADDR + 2, 0xC0);
        memory.advance_video_cycles(2);

        assert_eq!(memory.read_u16(TIMER0_ADDR), 0xFFFE);
        assert_eq!(memory.read_u16(IF_ADDR), 0x0008);
    }

    #[test]
    fn next_event_is_the_nearest_hblank_line_start_or_timer_overflow() {
        let mut memory = MemoryMap::new();
        assert_eq!(memory.cycles_until_next_event(), HBLANK_START_CYCLE);

        memory.advance_video_cycles(HBLANK_START_CYCLE);
        assert_eq!(
            memory.cycles_until_next_event(),
            CYCLES_PER_SCANLINE - HBLANK_START_CYCLE
        );

        memory.write_u16(TIMER0_ADDR, 0xFFF0);
        memory.write_u8(TIMER0_ADDR + 2, 0x80); // enable, prescaler 1
        assert_eq!(memory.cycles_until_next_event(), 16);

        memory.write_u8(TIMER0_ADDR + 2, 0x00);
        memory.write_u16(TIMER0_ADDR, 0xFFFF);
        memory.write_u8(TIMER0_ADDR + 2, 0x81); // enable, prescaler 64
        memory.advance_video_cycles(10);
        assert_eq!(memory.cycles_until_next_event(), 54);
    }

    #[test]
    fn vblank_entries_are_counted_once_per_frame() {
        let mut memory = MemoryMap::new();
        let vblank_start = VISIBLE_SCANLINES * CYCLES_PER_SCANLINE;

        memory.advance_video_cycles(vblank_start - 1);
        assert_eq!(memory.vblanks_started(), 0);
        memory.advance_video_cycles(1);
        assert_eq!(memory.vblanks_started(), 1);
        memory.advance_video_cycles(CYCLES_PER_FRAME * 2);
        assert_eq!(memory.vblanks_started(), 3);
    }

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
    fn keypad_interrupt_obeys_or_and_key_conditions() {
        let mut memory = MemoryMap::new();
        memory.write_u16(KEYCNT_ADDR, (1 << 14) | 0b11);
        memory.set_pressed_keys(0b01);
        assert_eq!(memory.read_u16(IF_ADDR), 1 << 12);

        memory.write_u16(IF_ADDR, 1 << 12);
        memory.write_u16(KEYCNT_ADDR, (1 << 15) | (1 << 14) | 0b11);
        memory.set_pressed_keys(0b01);
        assert_eq!(memory.read_u16(IF_ADDR), 0);
        memory.set_pressed_keys(0b11);
        assert_eq!(memory.read_u16(IF_ADDR), 1 << 12);
    }
}
