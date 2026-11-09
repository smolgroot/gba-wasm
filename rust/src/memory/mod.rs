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
pub const HBLANK_START_CYCLE: u32 = 1006;
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
const BIOS_END: u32 = 0x0000_3FFF;
/// BIOS opcode left in the read-protection latch at startup ([0xDC + 8]).
const BIOS_LATCH_AFTER_BOOT: u32 = 0xE129_F000;
/// BIOS opcode left in the latch when a software interrupt returns ([0x188 + 8]).
pub const BIOS_LATCH_AFTER_SWI: u32 = 0xE3A0_2004;
const DMA_TIMING_IMMEDIATE: u16 = 0;
const DMA_TIMING_VBLANK: u16 = 1;
const DMA_TIMING_HBLANK: u16 = 2;
const DMA_TIMING_SPECIAL: u16 = 3;
const SOUND_FIFO_A_ADDR: u32 = IO_BASE + 0x0A0;
/// Sound FIFO DMA always moves four words to the fixed FIFO address.
const SOUND_DMA_WORDS: u32 = 4;
/// Fixed setup cost per transfer; each unit adds a read and a write cycle.
const DMA_SETUP_CYCLES: u32 = 2;
const KEYINPUT_MASK: u16 = 0x03FF;
const BACKUP_START: u32 = 0x0E00_0000;
const EEPROM_START: u32 = 0x0D00_0000;
const EEPROM_START_LARGE_ROM: u32 = 0x0DFF_FF00;
const LARGE_ROM_SIZE: usize = 16 * 1024 * 1024;
const BACKUP_WINDOW_MASK: u32 = 0xFFFF;
const DISPLAY_MODE_MASK: u8 = 0x07;
const TILE_MODE_BG_VRAM_END: usize = 0x1_0000;
const BITMAP_MODE_BG_VRAM_END: usize = 0x1_4000;

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
        // Sound: lengths and frequencies are write-only; restart bits read back as zero.
        0x060 => 0x7F,
        0x062 | 0x068 => 0xC0,
        0x063 | 0x069 | 0x079 | 0x081 => 0xFF,
        0x065 | 0x06D | 0x075 | 0x07D => 0x40,
        0x070 => 0xE0,
        0x073 => 0xE0,
        0x07C => 0xFF,
        0x080 => 0x77,
        0x082 => 0x0F,
        0x083 => 0x77,
        0x088 => 0xFE,
        0x089 => 0xC3,
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

use crate::apu::Apu;
use crate::backup::{Backup, BackupKind};
use crate::state::impl_state;

mod dma;
mod timers;

use dma::DmaChannel;
use timers::Timer;

// The BIOS image and ROM are not state; held keys stay live; the volatile-read flag only
// feeds the idle-loop detector. The fetch address is set again by the next instruction, and
// the BIOS latch is left out so states from before it was modeled still load.
impl_state!(MemoryMap {
    backup, apu, ewram, iwram, vram, oam, palette, io, video_cycles, vblanks_started, timers,
    dma, affine_reference_writes,
} skip { bios, cartridge, pressed_keys, volatile_read, fetch, bios_latch });

#[derive(Debug, Clone)]
pub struct MemoryMap {
    pub bios: Vec<u8>,
    pub backup: Backup,
    pub apu: Apu,
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
    affine_reference_writes: [bool; 2],
    /// Set by reads of registers that change on every cycle (timer counters, sound status),
    /// which make a polling loop's outcome depend on time between events.
    volatile_read: std::cell::Cell<bool>,
    /// Address and state of the instruction being executed, which decide what BIOS and
    /// unmapped reads return.
    fetch: OpcodeFetch,
    /// Last opcode fetched from the BIOS. Reads of the BIOS from outside it return this.
    bios_latch: u32,
}

#[derive(Debug, Clone, Copy)]
struct OpcodeFetch {
    address: u32,
    thumb: bool,
}

impl MemoryMap {
    pub fn new() -> Self {
        let mut memory = Self {
            bios: crate::bios::hle_image(),
            backup: Backup::new(BackupKind::None),
            apu: Apu::new(),
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
            affine_reference_writes: [false; 2],
            volatile_read: std::cell::Cell::new(false),
            fetch: OpcodeFetch {
                address: 0,
                thumb: false,
            },
            bios_latch: BIOS_LATCH_AFTER_BOOT,
        };
        memory.reset_io_registers();
        memory
    }

    /// Maps a cartridge image and creates empty backup memory of the type its SDK library
    /// string declares.
    pub fn load_cartridge(&mut self, data: &[u8]) {
        self.cartridge.clear();
        self.cartridge.extend_from_slice(data);
        self.backup = Backup::new(BackupKind::detect(data));
    }

    pub fn advance_video_cycles(&mut self, cycles: u32) {
        let overflows = self.advance_timers(u64::from(cycles));
        for (timer, count) in overflows.into_iter().enumerate() {
            let refill = self.apu.timer_overflowed(&self.io, timer, count);
            for (fifo, needed) in refill.into_iter().enumerate() {
                if needed {
                    self.request_sound_dma(fifo);
                }
            }
        }
        self.apu.advance(&mut self.io, cycles);
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

    /// Clears I/O state to the values the BIOS leaves behind: all zero except the identity
    /// scale (1.0 in 8.8 fixed point) in the BG2/BG3 affine `PA` and `PD` parameters.
    pub fn reset_io_registers(&mut self) {
        self.reset_io_groups(true, true, true);
    }

    /// `RegisterRamReset` register groups: serial (`0x120`-`0x15F`), sound (`0x060`-`0x0AF`),
    /// and every other register, including display, DMA, timers, keypad, and interrupts.
    pub fn reset_io_groups(&mut self, serial: bool, sound: bool, other: bool) {
        for (offset, byte) in self.io.iter_mut().enumerate() {
            let reset = match offset {
                0x120..=0x15F => serial,
                0x060..=0x0AF => sound,
                _ => other,
            };
            if reset {
                *byte = 0;
            }
        }
        if sound {
            self.apu = Apu::new();
            self.io[0x089] = 0x02; // SOUNDBIAS level 0x200
        }
        if other {
            for offset in [0x020, 0x026, 0x030, 0x036] {
                self.io[offset + 1] = 0x01;
            }
            self.timers = [Timer::default(); 4];
            self.dma = [DmaChannel::default(); 4];
            self.affine_reference_writes = [true; 2];
        }
    }

    /// EEPROM occupies the top ROM window: all of `0x0D000000`-`0x0DFFFFFF` for ROMs up to
    /// 16 MiB, and only its last 256 bytes for larger ROMs.
    fn in_eeprom_window(&self, addr: u32) -> bool {
        let start = if self.cartridge.len() > LARGE_ROM_SIZE {
            EEPROM_START_LARGE_ROM
        } else {
            EEPROM_START
        };
        self.backup.kind() == BackupKind::Eeprom && (start..BACKUP_START).contains(&addr)
    }

    /// Position of the display in the current frame, in cycles from the start of scanline 0.
    pub fn frame_cycle(&self) -> u32 {
        self.video_cycles
    }

    /// Reports (and clears) whether the BG2/BG3 affine reference point registers were written
    /// since the last call. Writes reload the PPU's internal reference point.
    pub fn take_affine_reference_writes(&mut self) -> [bool; 2] {
        std::mem::take(&mut self.affine_reference_writes)
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

    /// Reports (and clears) whether a time-dependent register was read since the last call.
    pub fn take_volatile_read(&self) -> bool {
        self.volatile_read.replace(false)
    }

    fn read_io(&self, offset: usize) -> u8 {
        if (0x100..0x110).contains(&offset) || offset == 0x084 {
            self.volatile_read.set(true);
        }
        if (0x100..0x110).contains(&offset) {
            return self.timer_io_byte(offset);
        }

        let keyinput = !self.pressed_keys & KEYINPUT_MASK;
        match offset {
            0x000 => return self.io[offset] & DISPCNT_LOW_WRITABLE_MASK,
            0x001 => return self.io[offset],
            0x130 => return keyinput as u8,
            0x131 => return (keyinput >> 8) as u8,
            0x084 => return (self.io[offset] & 0x80) | self.apu.status(),
            0x090..=0x09F => return self.apu.wave_ram(&self.io, offset - 0x090),
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
            0x090..=0x09F => self.apu.write_wave_ram(&self.io, offset - 0x090, value),
            0x0A0..=0x0A7 => self.apu.push_fifo((offset - 0x0A0) / 4, value),
            0x084 => {
                self.io[offset] = value & 0x80;
                self.apu.register_written(&mut self.io, offset);
            }
            // PSG registers ignore writes while the sound master enable is off.
            0x060..=0x081 if self.io[0x084] & 0x80 == 0 => {}
            0x060..=0x089 => {
                self.io[offset] = value;
                self.apu.register_written(&mut self.io, offset);
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
            0x028..=0x02F | 0x038..=0x03F => {
                self.io[offset] = value;
                self.affine_reference_writes[(offset - 0x028) / 0x10] = true;
            }
            _ => self.io[offset] = value,
        }
        if matches!(offset, 0x132 | 0x133)
            && !key_condition_was_matched
            && self.key_condition_matches(self.pressed_keys)
        {
            self.request_keypad_interrupt();
        }
    }

    /// Records the instruction about to execute. Code running in the BIOS updates the
    /// BIOS latch with the opcode it prefetches ([pc + 8]).
    pub fn note_opcode_fetch(&mut self, address: u32, thumb: bool) {
        self.fetch = OpcodeFetch { address, thumb };
        if address <= BIOS_END && !thumb {
            self.bios_latch = self.bios_word((address & !3).wrapping_add(8));
        }
    }

    pub fn set_bios_latch(&mut self, value: u32) {
        self.bios_latch = value;
    }

    fn bios_word(&self, addr: u32) -> u32 {
        let offset = (addr & BIOS_END & !3) as usize;
        u32::from_le_bytes(self.bios[offset..offset + 4].try_into().unwrap())
    }

    /// The BIOS can only be read by code running inside it; other reads see the latch.
    fn read_bios_u8(&self, addr: u32) -> u8 {
        if self.fetch.address <= BIOS_END {
            self.bios[addr as usize]
        } else {
            (self.bios_latch >> ((addr & 3) * 8)) as u8
        }
    }

    /// Unmapped reads return the opcode the CPU prefetched last (GBATEK "Unpredictable
    /// Things"). ARM code leaves [pc + 8] on the bus; THUMB code leaves [pc + 4] in both
    /// halves, except in the BIOS, OAM, and IWRAM, which keep the neighbouring halfword in
    /// one half (the old bus value in IWRAM, approximated by [pc + 2]).
    fn open_bus_u8(&self, addr: u32) -> u8 {
        let OpcodeFetch { address: pc, thumb } = self.fetch;
        let value = if !thumb {
            self.read_mapped_u32((pc & !3).wrapping_add(8))
        } else {
            let halfword = |offset: u32| u32::from(self.read_mapped_u16(pc.wrapping_add(offset)));
            let aligned = pc & 2 == 0;
            match pc >> 24 {
                0x00 | 0x07 if aligned => halfword(4) | halfword(6) << 16,
                0x00 | 0x07 => halfword(2) | halfword(4) << 16,
                0x03 if aligned => halfword(4) | halfword(2) << 16,
                0x03 => halfword(2) | halfword(4) << 16,
                _ => halfword(4) * 0x0001_0001,
            }
        };
        (value >> ((addr & 3) * 8)) as u8
    }

    /// Opcode reads for the open-bus value. Only memory that can hold code is read, so an
    /// unmapped program counter cannot recurse into the open bus and I/O is not touched.
    fn read_mapped_u16(&self, addr: u32) -> u16 {
        let addr = addr & !1;
        let byte = |addr: u32| match addr {
            0x0000_0000..=BIOS_END => self.bios[addr as usize],
            0x0200_0000..=0x03FF_FFFF | 0x0500_0000..=0x0DFF_FFFF => self.read_u8(addr),
            _ => 0,
        };
        u16::from(byte(addr)) | u16::from(byte(addr + 1)) << 8
    }

    fn read_mapped_u32(&self, addr: u32) -> u32 {
        u32::from(self.read_mapped_u16(addr)) | u32::from(self.read_mapped_u16(addr + 2)) << 16
    }

    pub fn read_u8(&self, addr: u32) -> u8 {
        match addr {
            0x0000_0000..=BIOS_END => self.read_bios_u8(addr),
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
            BACKUP_START..=0x0FFF_FFFF => self.backup.read((addr & BACKUP_WINDOW_MASK) as usize),
            _ => self.open_bus_u8(addr),
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
            BACKUP_START..=0x0FFF_FFFF => self
                .backup
                .write((addr & BACKUP_WINDOW_MASK) as usize, value),
            _ => {}
        }
    }

    pub fn read_u16(&self, addr: u32) -> u16 {
        if self.in_eeprom_window(addr) {
            return self.backup.peek_eeprom_bit();
        }
        // Backup memory has an 8-bit bus: wide reads repeat the addressed byte.
        if (BACKUP_START..=0x0FFF_FFFF).contains(&addr) {
            return u16::from(self.read_u8(addr)) * 0x0101;
        }
        let aligned_addr = addr & !1;
        let lo = self.read_u8(aligned_addr) as u16;
        let hi = self.read_u8(aligned_addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    pub fn read_u32(&self, addr: u32) -> u32 {
        if (BACKUP_START..=0x0FFF_FFFF).contains(&addr) {
            return u32::from(self.read_u8(addr)) * 0x0101_0101;
        }
        let aligned_addr = addr & !3;
        u32::from(self.read_u16(aligned_addr))
            | (u32::from(self.read_u16(aligned_addr.wrapping_add(2))) << 16)
    }

    pub fn write_u16(&mut self, addr: u32, value: u16) {
        if self.in_eeprom_window(addr) {
            return self.backup.write_eeprom_bit(value);
        }
        // Backup memory has an 8-bit bus: only the byte lane of the address is stored.
        if addr >= BACKUP_START {
            return self.store_u8(addr, (value >> ((addr & 1) * 8)) as u8);
        }
        let aligned_addr = addr & !1;
        self.store_u8(aligned_addr, (value & 0xFF) as u8);
        self.store_u8(aligned_addr.wrapping_add(1), (value >> 8) as u8);
    }

    pub fn write_u32(&mut self, addr: u32, value: u32) {
        if addr >= BACKUP_START {
            return self.store_u8(addr, (value >> ((addr & 3) * 8)) as u8);
        }
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
    use crate::cpu::ROM_BASE;

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
        assert_eq!(memory.read_u8(0x0400_0400), memory.read_u8(0x1000_0000));
    }

    #[test]
    fn bios_reads_from_outside_the_bios_return_the_last_bios_opcode() {
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&[0; 0x10]);
        let image_word = u32::from_le_bytes(memory.bios[0..4].try_into().unwrap());

        assert_eq!(memory.read_u32(0x0000_0000), image_word);
        memory.note_opcode_fetch(ROM_BASE, true);
        assert_eq!(memory.read_u32(0x0000_0000), BIOS_LATCH_AFTER_BOOT);
        assert_eq!(
            memory.read_u8(0x0000_1001),
            (BIOS_LATCH_AFTER_BOOT >> 8) as u8
        );

        // The IRQ dispatcher's last fetch before returning is [0x13C + 8].
        memory.note_opcode_fetch(0x0000_013C, false);
        let prefetched = u32::from_le_bytes(memory.bios[0x144..0x148].try_into().unwrap());
        memory.note_opcode_fetch(ROM_BASE, true);
        assert_eq!(memory.read_u32(0x0000_0000), prefetched);
    }

    #[test]
    fn unmapped_reads_return_the_prefetched_opcode() {
        let mut memory = MemoryMap::new();
        let mut rom = vec![0; 0x20];
        rom[0x08..0x0C].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        rom[0x0C..0x10].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        memory.load_cartridge(&rom);

        memory.note_opcode_fetch(ROM_BASE + 0x04, true);
        assert_eq!(memory.read_u32(0x1000_0000), 0x3344_3344);
        assert_eq!(memory.read_u16(0x0000_4002), 0x3344);
        assert_eq!(memory.read_u8(0xFA79_DE1B), 0x33);

        memory.note_opcode_fetch(ROM_BASE + 0x04, false);
        assert_eq!(memory.read_u32(0x1000_0000), 0x5566_7788);
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

    #[test]
    fn cartridge_backup_is_detected_and_uses_an_8_bit_bus() {
        let mut rom = vec![0; 0x200];
        rom.extend_from_slice(b"SRAM_V113");
        let mut memory = MemoryMap::new();
        memory.load_cartridge(&rom);

        memory.write_u32(0x0E00_0010, 0x1122_3344);
        assert_eq!(
            memory.read_u8(0x0E00_0010),
            0x44,
            "only the addressed lane is stored"
        );
        assert_eq!(memory.read_u8(0x0E00_0011), 0xFF);
        assert_eq!(memory.read_u16(0x0E00_0010), 0x4444);
        assert_eq!(memory.read_u32(0x0E00_0010), 0x4444_4444);
        assert_eq!(
            memory.read_u8(0x0F00_0010),
            0x44,
            "the 64 KiB window mirrors"
        );
        assert_eq!(memory.cartridge, rom, "ROM bytes are never modified");
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
