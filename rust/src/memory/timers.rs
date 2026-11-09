use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Timer {
    pub(super) reload: u16,
    pub(super) counter: u16,
    pub(super) control: u8,
    pub(super) prescaler_phase: u64,
}

impl Timer {
    pub(super) const ENABLE: u8 = 1 << 7;
    pub(super) const CASCADE: u8 = 1 << 2;
    pub(super) const IRQ_ENABLE: u8 = 1 << 6;

    pub(super) fn advance(&mut self, ticks: u64) -> u64 {
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

impl_state!(Timer {
    reload,
    counter,
    control,
    prescaler_phase
});

impl MemoryMap {
    /// Advances the timers and returns the overflow counts of timers 0 and 1, which clock
    /// the sound FIFOs.
    pub(super) fn advance_timers(&mut self, cycles: u64) -> [u64; 2] {
        let prescalers = [1u64, 64, 256, 1024];
        let mut prior_overflows = 0;
        let mut sound_timer_overflows = [0; 2];
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
            if index < 2 {
                sound_timer_overflows[index] = prior_overflows;
            }

            if prior_overflows > 0 && timer.control & Timer::IRQ_ENABLE != 0 {
                let pending = self.io[0x202] as u16 | (u16::from(self.io[0x203]) << 8);
                let pending = pending | (IRQ_TIMER0 << index);
                self.io[0x202] = pending as u8;
                self.io[0x203] = (pending >> 8) as u8;
            }
        }
        sound_timer_overflows
    }

    pub(super) fn timer_io_byte(&self, offset: usize) -> u8 {
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
}
