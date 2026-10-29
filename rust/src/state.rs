//! Save-state serialization: a versioned little-endian binary format without external
//! dependencies.
//!
//! Each stateful struct implements [`State`], usually through [`impl_state!`], which
//! destructures the struct exhaustively: adding a field without listing it (or explicitly
//! skipping it) is a compile error, so states cannot silently miss new emulator state.

use std::collections::VecDeque;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    Truncated,
    NotAState,
    UnsupportedVersion { found: u32, supported: u32 },
    DifferentCartridge,
    Invalid(&'static str),
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(formatter, "save state is truncated"),
            Self::NotAState => write!(formatter, "file is not a GBA-WASM save state"),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "save state version {found} is not supported (this build reads version {supported})"
            ),
            Self::DifferentCartridge => {
                write!(formatter, "save state belongs to a different cartridge")
            }
            Self::Invalid(what) => write!(formatter, "save state is corrupt: {what}"),
        }
    }
}

impl std::error::Error for StateError {}

#[derive(Debug, Default)]
pub struct StateWriter {
    bytes: Vec<u8>,
}

impl StateWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug)]
pub struct StateReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> StateReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], StateError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(StateError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(StateError::Truncated)?;
        self.position = end;
        Ok(bytes)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], StateError> {
        Ok(self.read_bytes(N)?.try_into().unwrap())
    }

    pub fn is_finished(&self) -> bool {
        self.position == self.bytes.len()
    }
}

pub trait State {
    fn save(&self, writer: &mut StateWriter);
    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError>;
}

macro_rules! impl_integer_state {
    ($($type:ty),*) => {$(
        impl State for $type {
            fn save(&self, writer: &mut StateWriter) {
                writer.write_bytes(&self.to_le_bytes());
            }

            fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
                *self = <$type>::from_le_bytes(reader.read_array()?);
                Ok(())
            }
        }
    )*};
}

impl_integer_state!(u8, u16, u32, u64, i8, i16, i32);

impl State for usize {
    fn save(&self, writer: &mut StateWriter) {
        (*self as u64).save(writer);
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut value = 0u64;
        value.load(reader)?;
        *self = usize::try_from(value).map_err(|_| StateError::Invalid("index out of range"))?;
        Ok(())
    }
}

impl State for bool {
    fn save(&self, writer: &mut StateWriter) {
        u8::from(*self).save(writer);
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        *self = match reader.read_array::<1>()?[0] {
            0 => false,
            1 => true,
            _ => return Err(StateError::Invalid("boolean")),
        };
        Ok(())
    }
}

impl<T: State, const N: usize> State for [T; N] {
    fn save(&self, writer: &mut StateWriter) {
        for item in self {
            item.save(writer);
        }
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        for item in self {
            item.load(reader)?;
        }
        Ok(())
    }
}

/// Fixed-size memory: the stored length must match the region being restored.
impl State for Vec<u8> {
    fn save(&self, writer: &mut StateWriter) {
        (self.len() as u32).save(writer);
        writer.write_bytes(self);
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut length = 0u32;
        length.load(reader)?;
        if length as usize != self.len() {
            return Err(StateError::Invalid("memory region size"));
        }
        let bytes = reader.read_bytes(self.len())?;
        self.copy_from_slice(bytes);
        Ok(())
    }
}

impl State for VecDeque<i8> {
    fn save(&self, writer: &mut StateWriter) {
        (self.len() as u32).save(writer);
        for value in self {
            value.save(writer);
        }
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut length = 0u32;
        length.load(reader)?;
        let bytes = reader.read_bytes(length as usize)?;
        self.clear();
        self.extend(bytes.iter().map(|byte| *byte as i8));
        Ok(())
    }
}

impl<T: State + Default> State for Option<T> {
    fn save(&self, writer: &mut StateWriter) {
        match self {
            None => false.save(writer),
            Some(value) => {
                true.save(writer);
                value.save(writer);
            }
        }
    }

    fn load(&mut self, reader: &mut StateReader) -> Result<(), StateError> {
        let mut present = false;
        present.load(reader)?;
        *self = if present {
            let mut value = T::default();
            value.load(reader)?;
            Some(value)
        } else {
            None
        };
        Ok(())
    }
}

/// Implements [`State`] for a struct by saving the listed fields in order. Fields listed
/// under `skip` are left unchanged on load. Every field must appear in one of the lists.
macro_rules! impl_state {
    ($type:ty { $($field:ident),* $(,)? } $(skip { $($skipped:ident),* $(,)? })?) => {
        impl $crate::state::State for $type {
            fn save(&self, writer: &mut $crate::state::StateWriter) {
                let Self { $($field,)* $($($skipped: _,)*)? } = self;
                $($crate::state::State::save($field, writer);)*
            }

            fn load(
                &mut self,
                reader: &mut $crate::state::StateReader,
            ) -> Result<(), $crate::state::StateError> {
                let Self { $($field,)* $($($skipped: _,)*)? } = self;
                $($crate::state::State::load($field, reader)?;)*
                Ok(())
            }
        }
    };
}

pub(crate) use impl_state;

/// FNV-1a hash identifying the cartridge a state was taken from.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq)]
    struct Sample {
        flag: bool,
        value: u32,
        values: [i16; 2],
        optional: Option<u8>,
        ignored: u8,
    }

    impl_state!(Sample { flag, value, values, optional } skip { ignored });

    #[test]
    fn round_trips_listed_fields_and_leaves_skipped_ones() {
        let original = Sample {
            flag: true,
            value: 0x1234_5678,
            values: [-2, 300],
            optional: Some(7),
            ignored: 9,
        };
        let mut writer = StateWriter::new();
        original.save(&mut writer);
        let bytes = writer.finish();

        let mut restored = Sample::default();
        let mut reader = StateReader::new(&bytes);
        restored.load(&mut reader).unwrap();

        assert!(reader.is_finished());
        assert_eq!(
            restored,
            Sample {
                ignored: 0,
                ..original
            }
        );
    }

    #[test]
    fn rejects_truncated_data_and_invalid_booleans() {
        let mut value = 0u32;
        assert_eq!(
            value.load(&mut StateReader::new(&[1, 2])),
            Err(StateError::Truncated)
        );
        let mut flag = false;
        assert_eq!(
            flag.load(&mut StateReader::new(&[2])),
            Err(StateError::Invalid("boolean"))
        );
    }
}
