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
const DISPCNT_WRITABLE_MASK: u16 = 0xFFF7;
const DISPSTAT_VBLANK: u8 = 1 << 0;
const DISPSTAT_HBLANK: u8 = 1 << 1;
const DISPSTAT_VCOUNT_MATCH: u8 = 1 << 2;
const HBLANK_START_CYCLE: u32 = 1006;
const IRQ_TIMER0: u16 = 1 << 3;
const IRQ_VBLANK: u16 = 1 << 0;
const IRQ_HBLANK: u16 = 1 << 1;
const IRQ_VCOUNT: u16 = 1 << 2;
const IRQ_KEYPAD: u16 = 1 << 12;
const KEYINPUT_MASK: u16 = 0x03FF;

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

fn vram_index(addr: u32) -> usize {
    let offset = ((addr - 0x0600_0000) as usize) & 0x1_FFFF;
    if offset >= 0x1_8000 {
        offset - 0x8000
    } else {
        offset
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
    pub ewram: Vec<u8>,
    pub iwram: Vec<u8>,
    pub vram: Vec<u8>,
    pub oam: Vec<u8>,
    pub palette: Vec<u8>,
    pub io: Vec<u8>,
    pub cartridge: Vec<u8>,
    video_cycles: u32,
    pressed_keys: u16,
    timers: [Timer; 4],
}

impl MemoryMap {
    pub fn new() -> Self {
        Self {
            ewram: vec![0; EWRAM_SIZE],
            iwram: vec![0; IWRAM_SIZE],
            vram: vec![0; VRAM_SIZE],
            oam: vec![0; OAM_SIZE],
            palette: vec![0; PALETTE_SIZE],
            io: vec![0; IO_SIZE],
            cartridge: Vec::new(),
            video_cycles: 0,
            pressed_keys: 0,
            timers: [Timer::default(); 4],
        }
    }

    pub fn load_cartridge(&mut self, data: &[u8]) {
        self.cartridge.clear();
        self.cartridge.extend_from_slice(data);
    }

    pub fn advance_video_cycles(&mut self, cycles: u32) {
        self.advance_timers(u64::from(cycles));
        self.advance_display_interrupts(self.video_cycles, u64::from(cycles));
        self.video_cycles = (self.video_cycles + cycles % CYCLES_PER_FRAME) % CYCLES_PER_FRAME;
    }

    pub fn reset_video_clock(&mut self) {
        self.video_cycles = 0;
    }

    pub fn reset_io_registers(&mut self) {
        self.io.fill(0);
        self.timers = [Timer::default(); 4];
    }

    pub fn set_pressed_keys(&mut self, pressed: u16) {
        let old_pressed = self.pressed_keys;
        self.pressed_keys = pressed & KEYINPUT_MASK;
        if !self.key_condition_matches(old_pressed) && self.key_condition_matches(self.pressed_keys)
        {
            self.request_keypad_interrupt();
        }
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
            0x000 => return self.io[offset],
            0x001 => return self.io[offset] & (DISPCNT_WRITABLE_MASK >> 8) as u8,
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
            0x000 => self.io[offset] = value,
            0x001 => self.io[offset] = value & (DISPCNT_WRITABLE_MASK >> 8) as u8,
            0x004 => {
                self.io[offset] = (self.io[offset] & !DISPSTAT_IRQ_ENABLE_MASK)
                    | (value & DISPSTAT_IRQ_ENABLE_MASK);
            }
            0x005 => self.io[offset] = value,
            0x006..=0x007 | 0x130..=0x131 | 0x209 => {}
            0x132 => self.io[offset] = value & 0xFF,
            0x133 => self.io[offset] = value & 0xC3,
            0x200 => self.io[offset] = value,
            0x201 => self.io[offset] = value & 0x3F,
            0x202 => self.io[offset] &= !value,
            0x203 => self.io[offset] &= !(value & 0x3F),
            0x204 => self.io[offset] = value,
            0x205 => self.io[offset] = value & 0x7F,
            0x208 => self.io[offset] = value & 1,
            0x100..=0x10F => unreachable!(),
            _ => {}
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

    pub fn write_u8(&mut self, addr: u32, value: u8) {
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
        self.write_u8(aligned_addr, (value & 0xFF) as u8);
        self.write_u8(aligned_addr.wrapping_add(1), (value >> 8) as u8);
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
        memory.write_u8(0x0601_8000, 0x5A);

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
}
