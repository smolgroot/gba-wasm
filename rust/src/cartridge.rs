use std::fmt;

pub const MAX_ROM_SIZE: usize = 32 * 1024 * 1024;
const HEADER_SIZE: usize = 0xC0;
const NINTENDO_LOGO: [u8; 156] = [
    0x24, 0xFF, 0xAE, 0x51, 0x69, 0x9A, 0xA2, 0x21, 0x3D, 0x84, 0x82, 0x0A, 0x84, 0xE4, 0x09, 0xAD,
    0x11, 0x24, 0x8B, 0x98, 0xC0, 0x81, 0x7F, 0x21, 0xA3, 0x52, 0xBE, 0x19, 0x93, 0x09, 0xCE, 0x20,
    0x10, 0x46, 0x4A, 0x4A, 0xF8, 0x27, 0x31, 0xEC, 0x58, 0xC7, 0xE8, 0x33, 0x82, 0xE3, 0xCE, 0xBF,
    0x85, 0xF4, 0xDF, 0x94, 0xCE, 0x4B, 0x09, 0xC1, 0x94, 0x56, 0x8A, 0xC0, 0x13, 0x72, 0xA7, 0xFC,
    0x9F, 0x84, 0x4D, 0x73, 0xA3, 0xCA, 0x9A, 0x61, 0x58, 0x97, 0xA3, 0x27, 0xFC, 0x03, 0x98, 0x76,
    0x23, 0x1D, 0xC7, 0x61, 0x03, 0x04, 0xAE, 0x56, 0xBF, 0x38, 0x84, 0x00, 0x40, 0xA7, 0x0E, 0xFD,
    0xFF, 0x52, 0xFE, 0x03, 0x6F, 0x95, 0x30, 0xF1, 0x97, 0xFB, 0xC0, 0x85, 0x60, 0xD6, 0x80, 0x25,
    0xA9, 0x63, 0xBE, 0x03, 0x01, 0x4E, 0x38, 0xE2, 0xF9, 0xA2, 0x34, 0xFF, 0xBB, 0x3E, 0x03, 0x44,
    0x78, 0x00, 0x90, 0xCB, 0x88, 0x11, 0x3A, 0x94, 0x65, 0xC0, 0x7C, 0x63, 0x87, 0xF0, 0x3C, 0xAF,
    0xD6, 0x25, 0xE4, 0x8B, 0x38, 0x0A, 0xAC, 0x72, 0x21, 0xD4, 0xF8, 0x07,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartridgeError {
    Empty,
    TooLarge { size: usize, maximum: usize },
    TruncatedHeader { size: usize, minimum: usize },
    InvalidNintendoLogo,
    InvalidFixedValue { value: u8 },
    InvalidHeaderChecksum { expected: u8, actual: u8 },
}

impl fmt::Display for CartridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "cartridge ROM is empty"),
            Self::TooLarge { size, maximum } => write!(
                formatter,
                "cartridge ROM size {size} bytes exceeds the {maximum}-byte limit"
            ),
            Self::TruncatedHeader { size, minimum } => write!(
                formatter,
                "cartridge header is truncated: got {size} bytes, need at least {minimum}"
            ),
            Self::InvalidNintendoLogo => write!(formatter, "invalid Nintendo logo field"),
            Self::InvalidFixedValue { value } => write!(
                formatter,
                "invalid header fixed value: expected 0x96, got 0x{value:02X}"
            ),
            Self::InvalidHeaderChecksum { expected, actual } => write!(
                formatter,
                "invalid header checksum: expected 0x{expected:02X}, got 0x{actual:02X}"
            ),
        }
    }
}

impl std::error::Error for CartridgeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeHeader {
    pub title: String,
    pub game_code: String,
    pub maker_code: String,
    pub version: u8,
}

impl CartridgeHeader {
    pub fn parse(rom: &[u8]) -> Result<Self, CartridgeError> {
        if rom.is_empty() {
            return Err(CartridgeError::Empty);
        }
        if rom.len() > MAX_ROM_SIZE {
            return Err(CartridgeError::TooLarge {
                size: rom.len(),
                maximum: MAX_ROM_SIZE,
            });
        }
        if rom.len() < HEADER_SIZE {
            return Err(CartridgeError::TruncatedHeader {
                size: rom.len(),
                minimum: HEADER_SIZE,
            });
        }
        if rom[0x04..0xA0] != NINTENDO_LOGO {
            return Err(CartridgeError::InvalidNintendoLogo);
        }
        if rom[0xB2] != 0x96 {
            return Err(CartridgeError::InvalidFixedValue { value: rom[0xB2] });
        }

        let checksum_sum = rom[0xA0..=0xBC]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        let expected_checksum = 0u8.wrapping_sub(checksum_sum).wrapping_sub(0x19);
        if rom[0xBD] != expected_checksum {
            return Err(CartridgeError::InvalidHeaderChecksum {
                expected: expected_checksum,
                actual: rom[0xBD],
            });
        }

        Ok(Self {
            title: header_text(&rom[0xA0..0xAC]),
            game_code: header_text(&rom[0xAC..0xB0]),
            maker_code: header_text(&rom[0xB0..0xB2]),
            version: rom[0xBC],
        })
    }

    pub fn summary(&self) -> String {
        format!(
            "{} [{}:{} v{}]",
            self.title, self.game_code, self.maker_code, self.version
        )
    }
}

fn header_text(bytes: &[u8]) -> String {
    let text: String = bytes
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '?'
            }
        })
        .collect();
    text.trim_end().to_owned()
}

#[cfg(test)]
pub(crate) fn synthetic_rom(code: [u8; 4], first_instruction: [u8; 4]) -> Vec<u8> {
    let mut rom = vec![0; 0xC4];
    rom[..4].copy_from_slice(&first_instruction);
    rom[0x04..0xA0].copy_from_slice(&NINTENDO_LOGO);
    rom[0xA0..0xAC].copy_from_slice(b"SYNTHETIC   ");
    rom[0xAC..0xB0].copy_from_slice(&code);
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    rom[0xBC] = 0;
    let checksum_sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(checksum_sum).wrapping_sub(0x19);
    rom
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_rom() -> Vec<u8> {
        synthetic_rom(*b"TEST", [0; 4])
    }

    #[test]
    fn parses_identifying_header_fields() {
        let header = CartridgeHeader::parse(&valid_rom()).unwrap();

        assert_eq!(header.title, "SYNTHETIC");
        assert_eq!(header.game_code, "TEST");
        assert_eq!(header.maker_code, "00");
        assert_eq!(header.version, 0);
    }

    #[test]
    fn rejects_empty_truncated_and_oversized_images() {
        assert_eq!(CartridgeHeader::parse(&[]), Err(CartridgeError::Empty));
        assert!(matches!(
            CartridgeHeader::parse(&[0; HEADER_SIZE - 1]),
            Err(CartridgeError::TruncatedHeader { .. })
        ));
        assert!(matches!(
            CartridgeHeader::parse(&vec![0; MAX_ROM_SIZE + 1]),
            Err(CartridgeError::TooLarge { .. })
        ));
    }

    #[test]
    fn reports_the_header_field_that_fails_validation() {
        let mut rom = valid_rom();
        rom[4] ^= 1;
        assert_eq!(
            CartridgeHeader::parse(&rom),
            Err(CartridgeError::InvalidNintendoLogo)
        );

        let mut rom = valid_rom();
        rom[0xB2] = 0;
        assert_eq!(
            CartridgeHeader::parse(&rom),
            Err(CartridgeError::InvalidFixedValue { value: 0 })
        );

        let mut rom = valid_rom();
        rom[0xBD] ^= 1;
        assert!(matches!(
            CartridgeHeader::parse(&rom),
            Err(CartridgeError::InvalidHeaderChecksum { .. })
        ));
    }
}
