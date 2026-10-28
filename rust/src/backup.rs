//! Cartridge backup memory (save data) mapped at `0x0E000000`.
//!
//! The type is detected from the library ID string the Nintendo SDK embeds in the ROM.
//! SRAM and Flash (64 KiB and 128 KiB) are modeled; EEPROM is not, and such cartridges
//! run without backup memory.

use std::fmt;

const SRAM_SIZE: usize = 32 * 1024;
const FLASH_BANK_SIZE: usize = 64 * 1024;
const FLASH_SECTOR_SIZE: usize = 4 * 1024;
const ERASED: u8 = 0xFF;
/// Manufacturer and device IDs reported in Flash ID mode (Panasonic and Sanyo chips).
const FLASH_64K_ID: [u8; 2] = [0x32, 0x1B];
const FLASH_128K_ID: [u8; 2] = [0x62, 0x13];

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

    /// Size of the emulated backup memory; 0 when it is absent or not modeled.
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
            Self::Eeprom => "EEPROM (not emulated)",
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
        }
    }

    pub fn kind(&self) -> BackupKind {
        self.kind
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Replaces the contents with previously saved data of the same size.
    pub fn load(&mut self, data: &[u8]) -> Result<(), BackupSizeMismatch> {
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
