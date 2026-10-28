//! Audio: four PSG channels (two squares, wave, noise) and two DirectSound FIFOs, mixed
//! to stereo 16-bit samples at 32,768 Hz (one sample every 512 CPU cycles).
//!
//! Sound registers live in the I/O array owned by `MemoryMap`; this module keeps the
//! internal channel state and is told about register writes, timer overflows, and elapsed
//! cycles. Mixing approximates the analog stage: the result is clamped to the hardware's
//! 10-bit range around the bias and scaled to 16 bits.

use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 32_768;
const CYCLES_PER_SAMPLE: u32 = 512;
/// The frame sequencer runs at 512 Hz and clocks length, sweep, and envelope units.
const FRAME_SEQUENCER_PERIOD: u32 = 32_768;
/// At most one second of stereo samples is kept if nobody drains the buffer.
const MAX_BUFFERED_VALUES: usize = SAMPLE_RATE as usize * 2;
const FIFO_CAPACITY: usize = 32;
const FIFO_REFILL_LEVEL: usize = 16;
/// Square duty patterns (12.5%, 25%, 50%, 75%), one bit per step.
const DUTY_PATTERNS: [u8; 4] = [0b0000_0001, 0b1000_0001, 0b1000_0111, 0b0111_1110];

fn io_halfword(io: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([io[offset], io[offset + 1]])
}

#[derive(Debug, Clone, Copy, Default)]
struct Envelope {
    volume: u8,
    step_time: u8,
    increase: bool,
    timer: u8,
}

impl Envelope {
    /// Loads the envelope from bits 8-15 of a channel's envelope register.
    fn restart(&mut self, register: u16) {
        self.volume = (register >> 12) as u8;
        self.increase = register & (1 << 11) != 0;
        self.step_time = ((register >> 8) & 7) as u8;
        self.timer = self.step_time;
    }

    /// The DAC is off when the initial volume is 0 and the envelope decreases.
    fn dac_enabled(register: u16) -> bool {
        register & 0xF800 != 0
    }

    fn clock(&mut self) {
        if self.step_time == 0 {
            return;
        }
        self.timer = self.timer.saturating_sub(1);
        if self.timer == 0 {
            self.timer = self.step_time;
            if self.increase && self.volume < 15 {
                self.volume += 1;
            } else if !self.increase && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Length {
    remaining: u16,
    enabled: bool,
}

impl Length {
    /// Returns false when the counter expires and the channel must stop.
    fn clock(&mut self) -> bool {
        if self.enabled && self.remaining > 0 {
            self.remaining -= 1;
            return self.remaining != 0;
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Square {
    enabled: bool,
    frequency: u16,
    duty: u8,
    step: u8,
    timer: i32,
    length: Length,
    envelope: Envelope,
    sweep_timer: u8,
    sweep_enabled: bool,
    shadow_frequency: u16,
}

impl Square {
    fn period(&self) -> i32 {
        (2048 - i32::from(self.frequency)) * 16
    }

    fn advance(&mut self, cycles: i32) {
        if !self.enabled {
            return;
        }
        self.timer -= cycles;
        while self.timer <= 0 {
            self.timer += self.period();
            self.step = (self.step + 1) & 7;
        }
    }

    fn output(&self) -> i32 {
        if !self.enabled {
            return 0;
        }
        let volume = i32::from(self.envelope.volume);
        if DUTY_PATTERNS[usize::from(self.duty)] >> self.step & 1 != 0 {
            volume
        } else {
            -volume
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Wave {
    enabled: bool,
    frequency: u16,
    position: u8,
    timer: i32,
    length: Length,
}

#[derive(Debug, Clone, Copy, Default)]
struct Noise {
    enabled: bool,
    lfsr: u16,
    timer: i32,
    length: Length,
    envelope: Envelope,
}

impl Noise {
    /// Cycles per LFSR step: 524,288 Hz / r / 2^(s+1), with r = 0 treated as 0.5.
    fn period(register: u16) -> Option<i32> {
        let ratio = i32::from(register & 7);
        let shift = u32::from((register >> 4) & 0xF);
        if shift >= 14 {
            return None;
        }
        let base = if ratio == 0 { 16 } else { 32 * ratio };
        Some(base << (shift + 1))
    }

    fn advance(&mut self, cycles: i32, register: u16) {
        let Some(period) = Noise::period(register) else {
            return;
        };
        if !self.enabled {
            return;
        }
        self.timer -= cycles;
        while self.timer <= 0 {
            self.timer += period;
            let bit = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr = (self.lfsr >> 1) | (bit << 14);
            if register & (1 << 3) != 0 {
                self.lfsr = (self.lfsr & !(1 << 6)) | (bit << 6);
            }
        }
    }

    fn output(&self) -> i32 {
        if !self.enabled {
            return 0;
        }
        let volume = i32::from(self.envelope.volume);
        if self.lfsr & 1 == 0 { volume } else { -volume }
    }
}

#[derive(Debug, Clone, Default)]
struct Fifo {
    queue: VecDeque<i8>,
    current: i8,
}

#[derive(Debug, Clone, Default)]
pub struct Apu {
    squares: [Square; 2],
    wave: Wave,
    noise: Noise,
    /// Two 16-byte banks of 4-bit samples; register access goes to the bank not playing.
    wave_ram: [u8; 32],
    fifos: [Fifo; 2],
    sample_cycles: u32,
    sequencer_cycles: u32,
    sequencer_step: u8,
    samples: Vec<i16>,
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }

    fn master_enabled(io: &[u8]) -> bool {
        io[0x084] & 0x80 != 0
    }

    /// Active-channel flags for the low bits of `SOUNDCNT_X`.
    pub fn status(&self) -> u8 {
        u8::from(self.squares[0].enabled)
            | u8::from(self.squares[1].enabled) << 1
            | u8::from(self.wave.enabled) << 2
            | u8::from(self.noise.enabled) << 3
    }

    /// Wave RAM byte as seen through `0x04000090`-`0x0400009F` (the bank not playing).
    pub fn wave_ram(&self, io: &[u8], offset: usize) -> u8 {
        self.wave_ram[Self::wave_access_index(io, offset)]
    }

    pub fn write_wave_ram(&mut self, io: &[u8], offset: usize, value: u8) {
        self.wave_ram[Self::wave_access_index(io, offset)] = value;
    }

    fn wave_access_index(io: &[u8], offset: usize) -> usize {
        let playing_bank = usize::from(io[0x070] >> 6 & 1);
        (1 - playing_bank) * 16 + offset
    }

    pub fn push_fifo(&mut self, fifo: usize, value: u8) {
        let queue = &mut self.fifos[fifo].queue;
        if queue.len() < FIFO_CAPACITY {
            queue.push_back(value as i8);
        }
    }

    /// Reacts to a write already stored in `io` at `offset`: restarts, length loads,
    /// FIFO resets, and the master-disable reset of the PSG registers.
    pub fn register_written(&mut self, io: &mut [u8], offset: usize) {
        match offset {
            0x062 => self.squares[0].length.remaining = 64 - u16::from(io[0x062] & 0x3F),
            0x063 => self.squares[0].duty = io[0x062] >> 6,
            0x068 => self.squares[1].length.remaining = 64 - u16::from(io[0x068] & 0x3F),
            0x069 => self.squares[1].duty = io[0x068] >> 6,
            0x072 => self.wave.length.remaining = 256 - u16::from(io[0x072]),
            0x078 => self.noise.length.remaining = 64 - u16::from(io[0x078] & 0x3F),
            0x064 | 0x065 | 0x06C | 0x06D => {
                let channel = usize::from(offset >= 0x06C);
                let control = io_halfword(io, if channel == 0 { 0x064 } else { 0x06C });
                let square = &mut self.squares[channel];
                square.frequency = control & 0x7FF;
                square.length.enabled = control & (1 << 14) != 0;
                square.duty = io[if channel == 0 { 0x062 } else { 0x068 }] >> 6;
                if offset & 1 != 0 && control & (1 << 15) != 0 {
                    self.restart_square(io, channel);
                }
            }
            0x070 if io[0x070] & 0x80 == 0 => self.wave.enabled = false,
            0x074 | 0x075 => {
                let control = io_halfword(io, 0x074);
                self.wave.frequency = control & 0x7FF;
                self.wave.length.enabled = control & (1 << 14) != 0;
                if offset == 0x075 && control & (1 << 15) != 0 {
                    self.wave.enabled = io[0x070] & 0x80 != 0;
                    self.wave.position = 0;
                    self.wave.timer = (2048 - i32::from(self.wave.frequency)) * 8;
                    if self.wave.length.remaining == 0 {
                        self.wave.length.remaining = 256;
                    }
                }
            }
            0x07C | 0x07D => {
                let control = io_halfword(io, 0x07C);
                self.noise.length.enabled = control & (1 << 14) != 0;
                if offset == 0x07D && control & (1 << 15) != 0 {
                    let envelope = io_halfword(io, 0x078);
                    self.noise.enabled = Envelope::dac_enabled(envelope);
                    self.noise.envelope.restart(envelope);
                    self.noise.lfsr = if control & (1 << 3) != 0 {
                        0x7F
                    } else {
                        0x7FFF
                    };
                    self.noise.timer = Noise::period(control).unwrap_or(0);
                    if self.noise.length.remaining == 0 {
                        self.noise.length.remaining = 64;
                    }
                }
            }
            0x083 => {
                for (fifo, reset_bit) in [(0, 0x08), (1, 0x80)] {
                    if io[0x083] & reset_bit != 0 {
                        self.fifos[fifo].queue.clear();
                        self.fifos[fifo].current = 0;
                    }
                }
                io[0x083] &= 0x77; // the reset bits do not stay set
            }
            0x084 if !Self::master_enabled(io) => {
                io[0x060..=0x081].fill(0);
                self.squares = Default::default();
                self.wave = Wave::default();
                self.noise = Noise::default();
            }
            _ => {}
        }
        // A DAC turned off by an envelope write silences the channel immediately.
        for (channel, register) in [(0, 0x062), (1, 0x068)] {
            if offset == register + 1 && !Envelope::dac_enabled(io_halfword(io, register)) {
                self.squares[channel].enabled = false;
            }
        }
        if offset == 0x079 && !Envelope::dac_enabled(io_halfword(io, 0x078)) {
            self.noise.enabled = false;
        }
    }

    fn restart_square(&mut self, io: &mut [u8], channel: usize) {
        let envelope = io_halfword(io, if channel == 0 { 0x062 } else { 0x068 });
        let square = &mut self.squares[channel];
        square.enabled = Envelope::dac_enabled(envelope);
        square.envelope.restart(envelope);
        square.timer = square.period();
        if square.length.remaining == 0 {
            square.length.remaining = 64;
        }
        if channel == 0 {
            let sweep = io[0x060];
            let square = &mut self.squares[0];
            square.shadow_frequency = square.frequency;
            square.sweep_timer = Self::sweep_time(sweep);
            square.sweep_enabled = sweep & 0x77 != 0;
            if sweep & 7 != 0 && Self::sweep_target(square.shadow_frequency, sweep) > 2047 {
                square.enabled = false;
            }
        }
    }

    fn sweep_time(sweep: u8) -> u8 {
        match (sweep >> 4) & 7 {
            0 => 8,
            time => time,
        }
    }

    fn sweep_target(frequency: u16, sweep: u8) -> u16 {
        let delta = frequency >> (sweep & 7);
        if sweep & 0x08 != 0 {
            frequency - delta
        } else {
            frequency + delta
        }
    }

    /// Pops one sample from each FIFO driven by `timer` per overflow. Returns which FIFOs
    /// fell to half capacity and need a sound DMA refill.
    pub fn timer_overflowed(&mut self, io: &[u8], timer: usize, overflows: u64) -> [bool; 2] {
        let mut refill = [false; 2];
        if overflows == 0 {
            return refill;
        }
        let control = io_halfword(io, 0x082);
        for (fifo, timer_bit) in [(0, 1 << 10), (1, 1 << 14)] {
            if usize::from(control & timer_bit != 0) != timer {
                continue;
            }
            let state = &mut self.fifos[fifo];
            for _ in 0..overflows {
                if let Some(sample) = state.queue.pop_front() {
                    state.current = sample;
                }
            }
            refill[fifo] = state.queue.len() <= FIFO_REFILL_LEVEL;
        }
        refill
    }

    pub fn advance(&mut self, io: &mut [u8], cycles: u32) {
        self.sample_cycles += cycles;
        while self.sample_cycles >= CYCLES_PER_SAMPLE {
            self.sample_cycles -= CYCLES_PER_SAMPLE;
            self.run_sample(io);
        }
    }

    fn run_sample(&mut self, io: &mut [u8]) {
        let cycles = CYCLES_PER_SAMPLE as i32;
        for square in &mut self.squares {
            square.advance(cycles);
        }
        self.advance_wave(io, cycles);
        self.noise.advance(cycles, io_halfword(io, 0x07C));

        self.sequencer_cycles += CYCLES_PER_SAMPLE;
        if self.sequencer_cycles >= FRAME_SEQUENCER_PERIOD {
            self.sequencer_cycles -= FRAME_SEQUENCER_PERIOD;
            self.clock_sequencer(io);
        }

        let (left, right) = self.mix(io);
        if self.samples.len() >= MAX_BUFFERED_VALUES {
            self.samples.drain(..2);
        }
        self.samples.extend([left, right]);
    }

    fn advance_wave(&mut self, io: &[u8], cycles: i32) {
        if !self.wave.enabled {
            return;
        }
        let length = if io[0x070] & (1 << 5) != 0 { 64 } else { 32 };
        let period = (2048 - i32::from(self.wave.frequency)) * 8;
        self.wave.timer -= cycles;
        while self.wave.timer <= 0 {
            self.wave.timer += period;
            self.wave.position = (self.wave.position + 1) % length;
        }
    }

    fn wave_output(&self, io: &[u8]) -> i32 {
        if !self.wave.enabled {
            return 0;
        }
        let playing_bank = usize::from(io[0x070] >> 6 & 1);
        let position = usize::from(self.wave.position);
        let bank = (playing_bank + position / 32) % 2;
        let byte = self.wave_ram[bank * 16 + (position % 32) / 2];
        let sample = if position % 2 == 0 {
            byte >> 4
        } else {
            byte & 0xF
        };
        let centered = i32::from(sample) * 2 - 15;
        let control = io_halfword(io, 0x072);
        if control & (1 << 15) != 0 {
            return centered * 3 / 4;
        }
        match (control >> 13) & 3 {
            0 => 0,
            1 => centered,
            2 => centered / 2,
            _ => centered / 4,
        }
    }

    fn clock_sequencer(&mut self, io: &mut [u8]) {
        self.sequencer_step = (self.sequencer_step + 1) & 7;
        if self.sequencer_step & 1 == 0 {
            for square in &mut self.squares {
                square.enabled &= square.length.clock();
            }
            self.wave.enabled &= self.wave.length.clock();
            self.noise.enabled &= self.noise.length.clock();
        }
        if self.sequencer_step == 2 || self.sequencer_step == 6 {
            self.clock_sweep(io);
        }
        if self.sequencer_step == 7 {
            for square in &mut self.squares {
                square.envelope.clock();
            }
            self.noise.envelope.clock();
        }
    }

    fn clock_sweep(&mut self, io: &mut [u8]) {
        let sweep = io[0x060];
        let square = &mut self.squares[0];
        if !square.sweep_enabled {
            return;
        }
        square.sweep_timer = square.sweep_timer.saturating_sub(1);
        if square.sweep_timer != 0 {
            return;
        }
        square.sweep_timer = Self::sweep_time(sweep);
        if (sweep >> 4) & 7 == 0 {
            return;
        }
        let target = Self::sweep_target(square.shadow_frequency, sweep);
        if target > 2047 {
            square.enabled = false;
        } else if sweep & 7 != 0 {
            square.shadow_frequency = target;
            square.frequency = target;
            // The new frequency is written back to SOUND1CNT_X.
            let control = (io_halfword(io, 0x064) & !0x7FF) | target;
            io[0x064..0x066].copy_from_slice(&control.to_le_bytes());
            if Self::sweep_target(target, sweep) > 2047 {
                square.enabled = false;
            }
        }
    }

    fn mix(&self, io: &[u8]) -> (i16, i16) {
        if !Self::master_enabled(io) {
            return (0, 0);
        }
        let psg_control = io_halfword(io, 0x080);
        let control = io_halfword(io, 0x082);
        let outputs = [
            self.squares[0].output(),
            self.squares[1].output(),
            self.wave_output(io),
            self.noise.output(),
        ];
        // PSG ratio: 25%, 50%, or 100% (3 is prohibited and treated as 100%).
        let psg_shift = match control & 3 {
            0 => 2,
            1 => 1,
            _ => 0,
        };
        let mut sides = [0i32; 2]; // right, left
        for (side, total) in sides.iter_mut().enumerate() {
            let enable_shift = 8 + side * 4;
            let psg: i32 = outputs
                .iter()
                .enumerate()
                .filter(|(channel, _)| psg_control & (1 << (enable_shift + channel)) != 0)
                .map(|(_, output)| output)
                .sum();
            let master_volume = i32::from((psg_control >> (side * 4)) & 7) + 1;
            // Channels swing ±volume around 0, twice the hardware's 0..volume range.
            *total = (psg * master_volume / 2) >> psg_shift;
            for (fifo, volume_bit, enable_base) in [(0, 1 << 2, 8), (1, 1 << 3, 12)] {
                if control & (1 << (enable_base + side)) != 0 {
                    let scale = if control & volume_bit != 0 { 4 } else { 2 };
                    *total += i32::from(self.fifos[fifo].current) * scale;
                }
            }
        }
        // The output stage is 10 bits around the bias: clamp, then widen to 16 bits.
        let widen = |value: i32| (value.clamp(-512, 511) << 6) as i16;
        (widen(sides[1]), widen(sides[0]))
    }

    /// Removes and returns buffered interleaved left/right samples.
    pub fn take_samples(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io_with_master_enabled() -> Vec<u8> {
        let mut io = vec![0; 0x400];
        io[0x084] = 0x80;
        io
    }

    fn write(apu: &mut Apu, io: &mut [u8], offset: usize, value: u16) {
        io[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        apu.register_written(io, offset);
        apu.register_written(io, offset + 1);
    }

    #[test]
    fn square_channel_plays_its_duty_cycle_at_the_programmed_frequency() {
        let mut apu = Apu::new();
        let mut io = io_with_master_enabled();
        write(&mut apu, &mut io, 0x080, 0x2277); // full master volume, square 2 both sides
        write(&mut apu, &mut io, 0x082, 2); // PSG 100%
        write(&mut apu, &mut io, 0x068, 0xF080); // volume 15, 50% duty
        // Frequency 2048 - 64: one duty step every 1024 cycles, i.e. two samples.
        write(&mut apu, &mut io, 0x06C, 0x8000 | (2048 - 64));

        apu.advance(&mut io, CYCLES_PER_SAMPLE * 32);
        let left: Vec<i16> = apu.take_samples().iter().step_by(2).copied().collect();

        // 50% duty: 8 steps = 16 samples per period, half high and half low.
        let highs = left.iter().filter(|sample| **sample > 0).count();
        assert_eq!(highs, 16);
        assert!(
            left.iter()
                .all(|sample| sample.unsigned_abs() == 15 * 8 / 2 * 64)
        );
        assert_eq!(apu.status(), 0b10);
    }

    #[test]
    fn length_counter_stops_a_channel_and_clears_its_status_bit() {
        let mut apu = Apu::new();
        let mut io = io_with_master_enabled();
        write(&mut apu, &mut io, 0x062, 0xF03F); // length 64 - 63 = 1 step
        write(&mut apu, &mut io, 0x064, 0xC000 | 1000); // restart, length enabled
        assert_eq!(apu.status(), 0b01);

        apu.advance(&mut io, FRAME_SEQUENCER_PERIOD * 2);

        assert_eq!(apu.status(), 0);
    }

    #[test]
    fn fifo_samples_advance_on_timer_overflow_and_request_refills() {
        let mut apu = Apu::new();
        let mut io = io_with_master_enabled();
        write(
            &mut apu,
            &mut io,
            0x082,
            (1 << 2) | (1 << 8) | (1 << 9) | (1 << 14),
        ); // FIFO A 100% on timer 0, FIFO B on timer 1
        for sample in [10i8, -20, 30] {
            apu.push_fifo(0, sample as u8);
        }

        assert_eq!(apu.timer_overflowed(&io, 0, 1), [true, false]);
        apu.advance(&mut io, CYCLES_PER_SAMPLE);
        assert_eq!(apu.take_samples(), vec![40 << 6, 40 << 6]);

        apu.timer_overflowed(&io, 0, 1);
        apu.advance(&mut io, CYCLES_PER_SAMPLE);
        assert_eq!(apu.take_samples(), vec![-80 << 6, -80 << 6]);
        assert_eq!(
            apu.timer_overflowed(&io, 1, 1),
            [false, true],
            "timer 1 drives the empty FIFO B, which asks for a refill"
        );
    }

    #[test]
    fn master_disable_silences_output_and_clears_psg_registers() {
        let mut apu = Apu::new();
        let mut io = io_with_master_enabled();
        write(&mut apu, &mut io, 0x068, 0xF080);
        write(&mut apu, &mut io, 0x06C, 0x8000);
        io[0x084] = 0;
        apu.register_written(&mut io, 0x084);

        apu.advance(&mut io, CYCLES_PER_SAMPLE);
        assert_eq!(apu.take_samples(), vec![0, 0]);
        assert_eq!(io[0x068], 0);
        assert_eq!(apu.status(), 0);
    }

    #[test]
    fn wave_ram_access_uses_the_bank_that_is_not_playing() {
        let mut apu = Apu::new();
        let mut io = io_with_master_enabled();
        apu.write_wave_ram(&io, 0, 0xAB); // bank 0 plays, so this writes bank 1
        assert_eq!(apu.wave_ram[16], 0xAB);
        io[0x070] = 1 << 6; // play bank 1
        assert_eq!(apu.wave_ram(&io, 0), 0);
    }
}
