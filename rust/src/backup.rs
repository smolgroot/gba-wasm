//! Cartridge backup memory (save data) mapped at `0x0E000000`.
//!
//! The type is detected from the library ID string the Nintendo SDK embeds in the ROM.
//! SRAM, Flash (64 KiB and 128 KiB), and EEPROM (512 bytes and 8 KiB) are modeled.

use crate::state::{State, StateError, StateReader, StateWriter, impl_state};
use std::fmt;

const SRAM_SIZE: usize = 32 * 1024;
const FLASH_BANK_SIZE: usize = 64 * 1024;
const FLASH_SECTOR_SIZE: usize = 4 * 1024;
const ERASED: u8 = 0xFF;
/// Manufacturer and device IDs reported in Flash ID mode (Panasonic and Sanyo chips).
const FLASH_64K_ID: [u8; 2] = [0x32, 0x1B];
const FLASH_128K_ID: [u8; 2] = [0x62, 0x13];
const EEPROM_BLOCK_SIZE: usize = 8;
/// Bits returned for a read request: 4 ignored bits, then the 64-bit block.
const EEPROM_READ_BITS: u32 = 68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    None,
    Eeprom,
    Sram,
    Flash64K,
    Flash128K,
}

impl BackupKind {
    /// Detects the backup type from SDK library strings such as `FLASH1M_V103`.
    pub fn detect(rom: &[u8]) -> Self {
        let contains = |pattern: &[u8]| rom.windows(pattern.len()).any(|window| window == pattern);
        if contains(b"EEPROM_V") {
            Self::Eeprom
        } else if contains(b"SRAM_V") || contains(b"SRAM_F_V") {
            Self::Sram
        } else if contains(b"FLASH1M_V") {
            Self::Flash128K
        } else if contains(b"FLASH_V") || contains(b"FLASH512_V") {
            Self::Flash64K
        } else {
            Self::None
        }
    }

    /// Size of the emulated backup memory. EEPROM size is unknown (0) until the game's first
    /// DMA request reveals its address width.
    pub fn size(self) -> usize {
        match self {
            Self::None | Self::Eeprom => 0,
            Self::Sram => SRAM_SIZE,
            Self::Flash64K => FLASH_BANK_SIZE,
            Self::Flash128K => FLASH_BANK_SIZE * 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Eeprom => "EEPROM",
            Self::Sram => "SRAM",
            Self::Flash64K => "FLASH512",
            Self::Flash128K => "FLASH1M",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSizeMismatch {
    pub expected: usize,
    pub actual: usize,
}

impl fmt::Display for BackupSizeMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "save data is {} bytes, but this cartridge uses {} bytes",
            self.actual, self.expected
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum FlashCommand {
    #[default]
    None,
    Program,
    SelectBank,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EepromPhase {
    #[default]
    Command,
    Address {
        write: bool,
    },
    Data,
    Stop {
        write: bool,
    },
}

/// Serial EEPROM: the game streams one bit per halfword (bit 0) through DMA3.
/// Requests are `11` + address + stop bit (read) or `10` + address + 64 bits + stop (write).
#[derive(Debug, Clone, Default)]
struct EepromState {
    address_bits: Option<u32>,
    phase: EepromPhase,
    shift: u64,
    count: u32,
    address: usize,
    data: u64,
    /// Progress through the 68 bits of a pending read; `None` when idle (reads return 1).
    read_position: Option<u32>,
    read_block: u64,
}

impl EepromState {
    fn read_bit(&self) -> u16 {
        match self.read_position {
            Some(position) if position < 4 => 0,
            Some(position) => ((self.read_block >> (63 - (position - 4))) & 1) as u16,
            None => 1, // ready
        }
    }
}

impl State for FlashCommand {
    fn save(&self, writer: &mut StateWriter) {
        (*self as u8).save(writer);
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut tag = 0u8;
        tag.load(reader)?;
        *self = match tag {
            0 => Self::None,
            1 => Self::Program,
            2 => Self::SelectBank,
            _ => return Err(StateError::Invalid("flash command")),
        };
        Ok(())
    }
}

impl State for EepromPhase {
    fn save(&self, writer: &mut StateWriter) {
        let (tag, write) = match *self {
            Self::Command => (0u8, false),
            Self::Address { write } => (1, write),
            Self::Data => (2, false),
            Self::Stop { write } => (3, write),
        };
        tag.save(writer);
        write.save(writer);
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let (mut tag, mut write) = (0u8, false);
        tag.load(reader)?;
        write.load(reader)?;
        *self = match tag {
            0 => Self::Command,
            1 => Self::Address { write },
            2 => Self::Data,
            3 => Self::Stop { write },
            _ => return Err(StateError::Invalid("EEPROM phase")),
        };
        Ok(())
    }
}

impl_state!(EepromState {
    address_bits,
    phase,
    shift,
    count,
    address,
    data,
    read_position,
    read_block,
});

/// States carry the chip's command state but not its contents: loading a state never
/// rolls back the game's save file.
impl State for Backup {
    fn save(&self, writer: &mut StateWriter) {
        let Self {
            kind,
            data: _,
            dirty: _,
            unlock_stage,
            id_mode,
            erase_armed,
            pending,
            bank,
            eeprom,
        } = self;
        (*kind as u8).save(writer);
        unlock_stage.save(writer);
        id_mode.save(writer);
        erase_armed.save(writer);
        pending.save(writer);
        bank.save(writer);
        eeprom.save(writer);
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut kind = 0u8;
        kind.load(reader)?;
        if kind != self.kind as u8 {
            return Err(StateError::Invalid("backup type"));
        }
        self.unlock_stage.load(reader)?;
        self.id_mode.load(reader)?;
        self.erase_armed.load(reader)?;
        self.pending.load(reader)?;
        self.bank.load(reader)?;
        self.eeprom.load(reader)?;
        if self.bank > usize::from(self.kind == BackupKind::Flash128K) {
            return Err(StateError::Invalid("flash bank"));
        }
        // A state taken after the EEPROM size was detected may be loaded into a session that
        // has not detected it yet.
        if let Some(address_bits) = self.eeprom.address_bits {
            if !matches!(address_bits, 6 | 14) {
                return Err(StateError::Invalid("EEPROM size"));
            }
            let size = (1 << address_bits.min(10)) * EEPROM_BLOCK_SIZE;
            if self.data.len() != size {
                self.data.resize(size, ERASED);
            }
            if self.eeprom.address >= size / EEPROM_BLOCK_SIZE {
                return Err(StateError::Invalid("EEPROM address"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Backup {
    kind: BackupKind,
    data: Vec<u8>,
    dirty: bool,
    /// Progress through the `0xAA` to 0x5555, `0x55` to 0x2AAA unlock sequence.
    unlock_stage: u8,
    id_mode: bool,
    erase_armed: bool,
    pending: FlashCommand,
    bank: usize,
    eeprom: EepromState,
}

impl Backup {
    pub fn new(kind: BackupKind) -> Self {
        Self {
            kind,
            data: vec![ERASED; kind.size()],
            dirty: false,
            unlock_stage: 0,
            id_mode: false,
            erase_armed: false,
            pending: FlashCommand::None,
            bank: 0,
            eeprom: EepromState::default(),
        }
    }

    pub fn kind(&self) -> BackupKind {
        self.kind
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Replaces the contents with previously saved data of the same size. EEPROM data of
    /// 512 bytes or 8 KiB also sets the EEPROM size.
    pub fn load(&mut self, data: &[u8]) -> Result<(), BackupSizeMismatch> {
        if self.kind == BackupKind::Eeprom && self.data.is_empty() {
            let address_bits = match data.len() {
                512 => 6,
                8192 => 14,
                actual => {
                    return Err(BackupSizeMismatch {
                        expected: 8192,
                        actual,
                    });
                }
            };
            self.eeprom.address_bits = Some(address_bits);
            self.data = data.to_vec();
            self.dirty = false;
            return Ok(());
        }
        if data.len() != self.data.len() {
            return Err(BackupSizeMismatch {
                expected: self.data.len(),
                actual: data.len(),
            });
        }
        self.data.copy_from_slice(data);
        self.dirty = false;
        Ok(())
    }

    /// Reports whether the contents changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Uses the length of a DMA transfer to the EEPROM to set its address width: 9 or 73
    /// units for 6-bit (512 bytes), 17 or 81 units for 14-bit (8 KiB) addressing.
    pub fn observe_eeprom_dma_write(&mut self, units: u32) {
        if self.kind != BackupKind::Eeprom || self.eeprom.address_bits.is_some() {
            return;
        }
        let address_bits = match units {
            9 | 73 => 6,
            17 | 81 => 14,
            _ => return,
        };
        self.eeprom.address_bits = Some(address_bits);
        self.data = vec![ERASED; (1 << address_bits.min(10)) * EEPROM_BLOCK_SIZE];
    }

    /// Current EEPROM output bit without consuming it (CPU polling reads).
    pub fn peek_eeprom_bit(&self) -> u16 {
        self.eeprom.read_bit()
    }

    /// Next EEPROM output bit for a DMA read.
    pub fn read_eeprom_bit(&mut self) -> u16 {
        let bit = self.eeprom.read_bit();
        if let Some(position) = self.eeprom.read_position {
            self.eeprom.read_position = (position + 1 < EEPROM_READ_BITS).then_some(position + 1);
        }
        bit
    }

    pub fn write_eeprom_bit(&mut self, value: u16) {
        // Without a size, requests cannot be decoded; games always start with a DMA request.
        let Some(address_bits) = self.eeprom.address_bits else {
            return;
        };
        let state = &mut self.eeprom;
        state.shift = (state.shift << 1) | u64::from(value & 1);
        state.count += 1;
        match state.phase {
            EepromPhase::Command if state.count == 2 => {
                state.phase = match state.shift & 3 {
                    0b11 => EepromPhase::Address { write: false },
                    0b10 => EepromPhase::Address { write: true },
                    _ => EepromPhase::Command,
                };
            }
            EepromPhase::Address { write } if state.count == address_bits => {
                state.address =
                    (state.shift & 0x3FF) as usize % (self.data.len() / EEPROM_BLOCK_SIZE);
                state.phase = if write {
                    EepromPhase::Data
                } else {
                    EepromPhase::Stop { write: false }
                };
            }
            EepromPhase::Data if state.count == 64 => {
                state.data = state.shift;
                state.phase = EepromPhase::Stop { write: true };
            }
            EepromPhase::Stop { write } => {
                let start = state.address * EEPROM_BLOCK_SIZE;
                let block = &mut self.data[start..start + EEPROM_BLOCK_SIZE];
                if write {
                    block.copy_from_slice(&state.data.to_be_bytes());
                    self.dirty = true;
                } else {
                    state.read_block = u64::from_be_bytes(block.try_into().unwrap());
                    state.read_position = Some(0);
                }
                state.phase = EepromPhase::Command;
            }
            _ => return,
        }
        state.shift = 0;
        state.count = 0;
    }

    /// Reads one byte; `offset` is the address within the 64 KiB backup window.
    pub fn read(&self, offset: usize) -> u8 {
        match self.kind {
            BackupKind::Sram => self.data[offset % SRAM_SIZE],
            BackupKind::Flash64K | BackupKind::Flash128K => {
                if self.id_mode && offset < 2 {
                    let id = if self.kind == BackupKind::Flash64K {
                        FLASH_64K_ID
                    } else {
                        FLASH_128K_ID
                    };
                    id[offset]
                } else {
                    self.data[self.bank * FLASH_BANK_SIZE + offset]
                }
            }
            // Absent or unmodeled backup reads as an open data bus pulled high.
            BackupKind::None | BackupKind::Eeprom => ERASED,
        }
    }

    pub fn write(&mut self, offset: usize, value: u8) {
        match self.kind {
            BackupKind::Sram => {
                self.data[offset % SRAM_SIZE] = value;
                self.dirty = true;
            }
            BackupKind::Flash64K | BackupKind::Flash128K => self.write_flash(offset, value),
            BackupKind::None | BackupKind::Eeprom => {}
        }
    }

    fn write_flash(&mut self, offset: usize, value: u8) {
        match std::mem::take(&mut self.pending) {
            FlashCommand::Program => {
                // Programming can only clear bits of an erased byte.
                self.data[self.bank * FLASH_BANK_SIZE + offset] &= value;
                self.dirty = true;
                return;
            }
            FlashCommand::SelectBank if offset == 0 => {
                self.bank = usize::from(value & 1);
                return;
            }
            _ => {}
        }

        match (self.unlock_stage, offset, value) {
            (0, 0x5555, 0xAA) => self.unlock_stage = 1,
            (1, 0x2AAA, 0x55) => self.unlock_stage = 2,
            (2, _, _) => {
                self.unlock_stage = 0;
                self.flash_command(offset, value);
            }
            (_, _, 0xF0) => {
                self.unlock_stage = 0;
                self.id_mode = false;
            }
            _ => self.unlock_stage = 0,
        }
    }

    fn flash_command(&mut self, offset: usize, value: u8) {
        if std::mem::take(&mut self.erase_armed) {
            match (offset, value) {
                (0x5555, 0x10) => self.data.fill(ERASED),
                (_, 0x30) => {
                    let start = self.bank * FLASH_BANK_SIZE + (offset & !(FLASH_SECTOR_SIZE - 1));
                    self.data[start..start + FLASH_SECTOR_SIZE].fill(ERASED);
                }
                _ => return,
            }
            self.dirty = true;
            return;
        }
        if offset != 0x5555 {
            return;
        }
        match value {
            0x90 => self.id_mode = true,
            0xF0 => self.id_mode = false,
            0x80 => self.erase_armed = true,
            0xA0 => self.pending = FlashCommand::Program,
            0xB0 if self.kind == BackupKind::Flash128K => self.pending = FlashCommand::SelectBank,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(backup: &mut Backup, value: u8) {
        backup.write(0x5555, 0xAA);
        backup.write(0x2AAA, 0x55);
        backup.write(0x5555, value);
    }

    #[test]
    fn detects_backup_type_from_sdk_library_strings() {
        let rom_with = |id: &[u8]| {
            let mut rom = vec![0; 0x100];
            rom.extend_from_slice(id);
            rom
        };
        assert_eq!(
            BackupKind::detect(&rom_with(b"FLASH1M_V103")),
            BackupKind::Flash128K
        );
        assert_eq!(
            BackupKind::detect(&rom_with(b"FLASH_V123")),
            BackupKind::Flash64K
        );
        assert_eq!(
            BackupKind::detect(&rom_with(b"FLASH512_V131")),
            BackupKind::Flash64K
        );
        assert_eq!(
            BackupKind::detect(&rom_with(b"SRAM_V113")),
            BackupKind::Sram
        );
        assert_eq!(
            BackupKind::detect(&rom_with(b"EEPROM_V122")),
            BackupKind::Eeprom
        );
        assert_eq!(BackupKind::detect(&rom_with(b"nothing")), BackupKind::None);
    }

    #[test]
    fn sram_reads_back_writes_and_marks_dirty() {
        let mut backup = Backup::new(BackupKind::Sram);
        backup.write(0x1234, 0x56);

        assert_eq!(backup.read(0x1234), 0x56);
        assert_eq!(
            backup.read(0x9234),
            0x56,
            "32 KiB SRAM mirrors in the 64 KiB window"
        );
        assert!(backup.take_dirty());
        assert!(!backup.take_dirty());
    }

    #[test]
    fn flash_reports_its_chip_id_only_in_id_mode() {
        let mut backup = Backup::new(BackupKind::Flash128K);
        assert_eq!([backup.read(0), backup.read(1)], [ERASED, ERASED]);

        command(&mut backup, 0x90);
        assert_eq!([backup.read(0), backup.read(1)], FLASH_128K_ID);

        command(&mut backup, 0xF0);
        assert_eq!(backup.read(0), ERASED);
    }

    fn program(backup: &mut Backup, offset: usize, value: u8) {
        command(backup, 0xA0);
        backup.write(offset, value);
    }

    /// Erase prefix, then the unlock sequence and `0x30` at the sector address.
    fn erase_sector(backup: &mut Backup, sector: usize) {
        command(backup, 0x80);
        backup.write(0x5555, 0xAA);
        backup.write(0x2AAA, 0x55);
        backup.write(sector, 0x30);
    }

    #[test]
    fn flash_programs_bytes_and_erases_sectors_and_the_chip() {
        let mut backup = Backup::new(BackupKind::Flash64K);
        backup.write(0x1000, 0x12);
        assert_eq!(
            backup.read(0x1000),
            ERASED,
            "writes without a command are ignored"
        );

        program(&mut backup, 0x1000, 0x12);
        program(&mut backup, 0x2000, 0x34);
        assert_eq!(backup.read(0x1000), 0x12);
        assert!(backup.take_dirty());

        erase_sector(&mut backup, 0x1000);
        assert_eq!(backup.read(0x1000), ERASED);
        assert_eq!(backup.read(0x2000), 0x34, "other sectors are kept");

        command(&mut backup, 0x80);
        command(&mut backup, 0x10); // chip erase
        assert_eq!(backup.read(0x2000), ERASED);
        assert!(backup.take_dirty());
    }

    #[test]
    fn flash_128k_switches_banks() {
        let mut backup = Backup::new(BackupKind::Flash128K);
        command(&mut backup, 0xB0);
        backup.write(0, 1);
        command(&mut backup, 0xA0);
        backup.write(0x10, 0xAB);

        assert_eq!(backup.read(0x10), 0xAB);
        assert_eq!(backup.data()[FLASH_BANK_SIZE + 0x10], 0xAB);
        command(&mut backup, 0xB0);
        backup.write(0, 0);
        assert_eq!(backup.read(0x10), ERASED);
    }

    fn send_bits(backup: &mut Backup, value: u64, bits: u32) {
        for bit in (0..bits).rev() {
            backup.write_eeprom_bit(((value >> bit) & 1) as u16);
        }
    }

    #[test]
    fn eeprom_size_comes_from_the_first_request_length() {
        let mut small = Backup::new(BackupKind::Eeprom);
        assert!(small.data().is_empty());
        small.observe_eeprom_dma_write(9);
        assert_eq!(small.data().len(), 512);

        let mut large = Backup::new(BackupKind::Eeprom);
        large.observe_eeprom_dma_write(81);
        large.observe_eeprom_dma_write(9);
        assert_eq!(large.data().len(), 8192, "the first request decides");
    }

    #[test]
    fn eeprom_writes_and_reads_back_64_bit_blocks_msb_first() {
        let mut backup = Backup::new(BackupKind::Eeprom);
        backup.observe_eeprom_dma_write(81);
        let block = 0x0123_4567_89AB_CDEF;

        // Write request: 10, 14-bit address 5, 64 data bits, stop bit.
        send_bits(&mut backup, 0b10, 2);
        send_bits(&mut backup, 5, 14);
        send_bits(&mut backup, block, 64);
        send_bits(&mut backup, 0, 1);
        assert!(backup.take_dirty());
        assert_eq!(&backup.data()[40..48], &block.to_be_bytes());
        assert_eq!(backup.peek_eeprom_bit(), 1, "ready after the write");

        // Read request: 11, address 5, stop bit; then 4 ignored bits and the block.
        send_bits(&mut backup, 0b11, 2);
        send_bits(&mut backup, 5, 14);
        send_bits(&mut backup, 0, 1);
        let bits: Vec<u16> = (0..68).map(|_| backup.read_eeprom_bit()).collect();
        assert_eq!(&bits[..4], &[0, 0, 0, 0]);
        let value = bits[4..]
            .iter()
            .fold(0u64, |acc, bit| (acc << 1) | u64::from(*bit));
        assert_eq!(value, block);
        assert_eq!(backup.read_eeprom_bit(), 1, "idle again");
    }

    #[test]
    fn eeprom_accepts_saved_data_of_either_size_before_the_game_runs() {
        let mut backup = Backup::new(BackupKind::Eeprom);
        assert!(backup.load(&[0; 100]).is_err());
        backup.load(&vec![0x5A; 512]).unwrap();
        assert_eq!(backup.data().len(), 512);
        backup.observe_eeprom_dma_write(81);
        assert_eq!(backup.data().len(), 512, "loaded data keeps its size");
    }

    #[test]
    fn loading_saved_data_requires_the_matching_size() {
        let mut backup = Backup::new(BackupKind::Flash64K);
        assert_eq!(
            backup.load(&[0; 10]),
            Err(BackupSizeMismatch {
                expected: FLASH_BANK_SIZE,
                actual: 10
            })
        );

        let mut saved = vec![ERASED; FLASH_BANK_SIZE];
        saved[3] = 7;
        backup.load(&saved).unwrap();
        assert_eq!(backup.read(3), 7);
        assert!(!backup.take_dirty());
    }
}
