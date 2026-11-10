//! The mGBA debug-log registers, an opt-in device for test harnesses.
//!
//! Test ROMs such as the mGBA suite report results through registers that only exist in
//! mGBA, at the top of the I/O page (outside the 1 KiB hardware I/O window):
//!
//! | Address      | Size  | Meaning                                                    |
//! | ------------ | ----- | ---------------------------------------------------------- |
//! | `0x04FFF600` | 256 B | NUL-terminated message buffer                              |
//! | `0x04FFF700` | u16   | write with bit 8 set: emit the buffer at level `value & 7` |
//! | `0x04FFF780` | u16   | write `0xC0DE` to enable; reads back `0x1DEA` once enabled |
//!
//! The device is off unless the host turns it on. While it is off, these addresses behave as
//! the unmapped open bus they are on hardware, so enabling it is the only thing that changes
//! emulated behavior. Even when the host has turned it on, a ROM must complete the enable
//! handshake before the buffer and flags registers respond. The browser build never turns it
//! on. Its state is not part of a save state: the host owns it.

use std::collections::VecDeque;

pub const DEBUG_LOG_BASE: u32 = 0x04FF_F600;
pub const DEBUG_LOG_END: u32 = 0x04FF_F7FF;

const STRING_END: usize = 0x100;
const FLAGS: usize = 0x100;
const ENABLE: usize = 0x180;
const ENABLE_REQUEST: u16 = 0xC0DE;
const ENABLE_ANSWER: u16 = 0x1DEA;
const FLAGS_FLUSH: u16 = 1 << 8;
const LEVEL_MASK: u16 = 0x7;
/// Lines kept for the host; the oldest are dropped past this, so a host that never drains
/// the log cannot grow memory without bound.
const MAX_LINES: usize = 4096;

/// mGBA's log levels, as written in the low bits of the flags register.
pub const LOG_FATAL: u8 = 0;
pub const LOG_ERROR: u8 = 1;
pub const LOG_WARN: u8 = 2;
pub const LOG_INFO: u8 = 3;
pub const LOG_DEBUG: u8 = 4;

/// One message a ROM emitted through the flags register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLogLine {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DebugLog {
    /// Set by the host. Everything below is inert while this is false.
    available: bool,
    /// The raw value last written to the enable register.
    enable_register: u16,
    flags: u16,
    buffer: [u8; STRING_END],
    lines: VecDeque<DebugLogLine>,
}

impl DebugLog {
    pub fn new() -> Self {
        Self {
            available: false,
            enable_register: 0,
            flags: 0,
            buffer: [0; STRING_END],
            lines: VecDeque::new(),
        }
    }

    /// Turns the device on or off for the host. Turning it off discards its state.
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
        if !available {
            self.reset();
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Forgets the ROM's handshake, buffer, and any undrained lines (a new cartridge was
    /// loaded). The host's on/off setting stays.
    pub fn reset(&mut self) {
        self.enable_register = 0;
        self.flags = 0;
        self.buffer = [0; STRING_END];
        self.lines.clear();
    }

    /// Removes and returns the lines emitted since the last call.
    pub fn take_lines(&mut self) -> Vec<DebugLogLine> {
        self.lines.drain(..).collect()
    }

    fn enabled(&self) -> bool {
        self.available && self.enable_register == ENABLE_REQUEST
    }

    /// Reads one byte, or `None` where the address is open bus.
    pub fn read(&self, addr: u32) -> Option<u8> {
        if !self.enabled() {
            return None;
        }
        let offset = (addr - DEBUG_LOG_BASE) as usize;
        match offset {
            0..STRING_END => Some(self.buffer[offset]),
            ENABLE | 0x181 => Some(ENABLE_ANSWER.to_le_bytes()[offset - ENABLE]),
            _ => None,
        }
    }

    /// Stores one byte. Halfword and word stores arrive as consecutive bytes, low first.
    pub fn write(&mut self, addr: u32, value: u8) {
        if !self.available {
            return;
        }
        let offset = (addr - DEBUG_LOG_BASE) as usize;
        match offset {
            ENABLE | 0x181 => {
                let shift = (offset - ENABLE) * 8;
                self.enable_register =
                    self.enable_register & !(0xFF << shift) | u16::from(value) << shift;
            }
            _ if !self.enabled() => {}
            0..STRING_END => self.buffer[offset] = value,
            FLAGS => self.flags = self.flags & 0xFF00 | u16::from(value),
            0x101 => {
                self.flags = self.flags & 0x00FF | u16::from(value) << 8;
                if self.flags & FLAGS_FLUSH != 0 {
                    self.flush();
                }
            }
            _ => {}
        }
    }

    /// Emits the buffer up to its first NUL (or all 256 bytes) as one line.
    fn flush(&mut self) {
        let length = self
            .buffer
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(STRING_END);
        if self.lines.len() == MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(DebugLogLine {
            level: (self.flags & LEVEL_MASK) as u8,
            text: String::from_utf8_lossy(&self.buffer[..length]).into_owned(),
        });
    }
}

impl Default for DebugLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRING: u32 = DEBUG_LOG_BASE;
    const FLAGS_ADDR: u32 = DEBUG_LOG_BASE + 0x100;
    const ENABLE_ADDR: u32 = DEBUG_LOG_BASE + 0x180;

    fn write_u16(log: &mut DebugLog, addr: u32, value: u16) {
        let [low, high] = value.to_le_bytes();
        log.write(addr, low);
        log.write(addr + 1, high);
    }

    fn write_string(log: &mut DebugLog, text: &str) {
        for (index, byte) in text.bytes().chain([0]).enumerate() {
            log.write(STRING + index as u32, byte);
        }
    }

    fn enabled_log() -> DebugLog {
        let mut log = DebugLog::new();
        log.set_available(true);
        write_u16(&mut log, ENABLE_ADDR, ENABLE_REQUEST);
        log
    }

    #[test]
    fn handshake_reads_back_the_answer_only_after_the_request() {
        let mut log = DebugLog::new();
        log.set_available(true);
        assert_eq!(log.read(ENABLE_ADDR), None);

        write_u16(&mut log, ENABLE_ADDR, 0x1234);
        assert_eq!(log.read(ENABLE_ADDR), None);

        write_u16(&mut log, ENABLE_ADDR, ENABLE_REQUEST);
        assert_eq!(log.read(ENABLE_ADDR), Some(0xEA));
        assert_eq!(log.read(ENABLE_ADDR + 1), Some(0x1D));

        write_u16(&mut log, ENABLE_ADDR, 0);
        assert_eq!(log.read(ENABLE_ADDR), None);
    }

    #[test]
    fn handshake_is_not_answered_unless_the_host_made_the_device_available() {
        let mut log = DebugLog::new();
        write_u16(&mut log, ENABLE_ADDR, ENABLE_REQUEST);
        assert_eq!(log.read(ENABLE_ADDR), None);
        write_string(&mut log, "hidden");
        write_u16(&mut log, FLAGS_ADDR, FLAGS_FLUSH | u16::from(LOG_INFO));
        assert!(log.take_lines().is_empty());
    }

    #[test]
    fn flush_emits_the_buffer_up_to_its_nul_at_the_written_level() {
        let mut log = enabled_log();
        write_string(&mut log, "BEGIN: Memory");
        write_u16(&mut log, FLAGS_ADDR, FLAGS_FLUSH | u16::from(LOG_DEBUG));
        write_string(&mut log, "END: 3/4");
        write_u16(&mut log, FLAGS_ADDR, FLAGS_FLUSH | u16::from(LOG_WARN));

        assert_eq!(
            log.take_lines(),
            vec![
                DebugLogLine {
                    level: LOG_DEBUG,
                    text: "BEGIN: Memory".into()
                },
                DebugLogLine {
                    level: LOG_WARN,
                    text: "END: 3/4".into()
                },
            ]
        );
        assert!(log.take_lines().is_empty());
    }

    #[test]
    fn writes_without_the_flush_bit_emit_nothing() {
        let mut log = enabled_log();
        write_string(&mut log, "quiet");
        write_u16(&mut log, FLAGS_ADDR, u16::from(LOG_INFO));
        assert!(log.take_lines().is_empty());
    }

    #[test]
    fn a_full_buffer_without_a_nul_is_emitted_whole() {
        let mut log = enabled_log();
        for index in 0..0x100 {
            log.write(STRING + index, b'x');
        }
        write_u16(&mut log, FLAGS_ADDR, FLAGS_FLUSH);
        let lines = log.take_lines();
        assert_eq!(lines[0].text.len(), 0x100);
    }

    #[test]
    fn buffer_reads_back_while_enabled() {
        let mut log = enabled_log();
        write_string(&mut log, "ab");
        assert_eq!(log.read(STRING), Some(b'a'));
        assert_eq!(log.read(STRING + 1), Some(b'b'));
        assert_eq!(log.read(STRING + 2), Some(0));
        assert_eq!(log.read(FLAGS_ADDR), None);
    }

    #[test]
    fn reset_forgets_the_handshake_but_keeps_the_host_setting() {
        let mut log = enabled_log();
        write_string(&mut log, "old");
        write_u16(&mut log, FLAGS_ADDR, FLAGS_FLUSH);
        log.reset();
        assert!(log.is_available());
        assert_eq!(log.read(ENABLE_ADDR), None);
        assert!(log.take_lines().is_empty());

        log.set_available(false);
        assert!(!log.is_available());
    }

    #[test]
    fn undrained_lines_are_bounded() {
        let mut log = enabled_log();
        for index in 0..MAX_LINES + 10 {
            write_string(&mut log, &index.to_string());
            write_u16(&mut log, FLAGS_ADDR, FLAGS_FLUSH);
        }
        let lines = log.take_lines();
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines[0].text, "10");
    }
}
