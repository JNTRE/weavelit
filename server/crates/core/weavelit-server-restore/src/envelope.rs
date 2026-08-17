use crate::EnvelopeError;

/// Fixed 8-byte magic that begins every Weavelit backup artifact.
pub const BACKUP_MAGIC: [u8; 8] = *b"WLBKUP\r\n";

/// The only outer backup format version this Server accepts.
pub const BACKUP_FORMAT_VERSION: u16 = 1;

/// Byte length of the fixed outer header.
pub const HEADER_LENGTH: usize = 20;

const MAGIC_RANGE: std::ops::Range<usize> = 0..8;
const FLAGS_RANGE: std::ops::Range<usize> = 10..12;
const LENGTH_RANGE: std::ops::Range<usize> = 12..20;

/// Validated outer envelope and the age v1 stream it frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Envelope<'artifact> {
    format_version: u16,
    payload: &'artifact [u8],
}

impl<'artifact> Envelope<'artifact> {
    /// Parses and validates the fixed outer header of an artifact.
    ///
    /// Format version 1 defines no compression, so the declared
    /// encrypted-payload length must equal the remaining stream length exactly.
    pub fn parse(artifact: &'artifact [u8]) -> Result<Self, EnvelopeError> {
        if artifact.len() < HEADER_LENGTH {
            return Err(EnvelopeError::TooShort);
        }
        if artifact[MAGIC_RANGE] != BACKUP_MAGIC {
            return Err(EnvelopeError::MagicMismatch);
        }

        let format_version = u16::from_be_bytes([artifact[8], artifact[9]]);
        if format_version != BACKUP_FORMAT_VERSION {
            return Err(EnvelopeError::UnsupportedFormatVersion);
        }
        if artifact[FLAGS_RANGE] != [0, 0] {
            return Err(EnvelopeError::FlagsNotZero);
        }

        let declared_length = u64::from_be_bytes(
            artifact[LENGTH_RANGE]
                .try_into()
                .expect("the declared length field is exactly eight bytes"),
        );
        let payload = &artifact[HEADER_LENGTH..];
        if declared_length != payload.len() as u64 {
            return Err(EnvelopeError::DeclaredLengthMismatch);
        }

        Ok(Self {
            format_version,
            payload,
        })
    }

    /// Returns the validated outer format version.
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Returns the framed age v1 stream.
    pub const fn payload(&self) -> &'artifact [u8] {
        self.payload
    }
}
