use crate::crypto::KEY_SIZE;
use heapless::String;
use sequential_storage::map::{SerializationError, Value};

const CONTACT_RECORD_BYTES: usize = 16 + KEY_SIZE + 32 + 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub device_id: [u8; 16],
    pub key: [u8; KEY_SIZE],
    pub custom_name: String<32>,
    /// Blocked contacts are dropped on receive and hidden from both lists.
    pub blocked: bool,
}

impl Contact {
    /// Uses the custom label or the device ID as uppercase hexadecimal.
    pub fn display_name(&self) -> String<32> {
        if self.custom_name.is_empty() {
            let mut name = String::<32>::new();
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for byte in self.device_id {
                let _ = name.push(HEX[(byte >> 4) as usize] as char);
                let _ = name.push(HEX[(byte & 0x0F) as usize] as char);
            }
            name
        } else {
            self.custom_name.clone()
        }
    }
}

impl<'a> Value<'a> for Contact {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        if buffer.len() < CONTACT_RECORD_BYTES {
            return Err(SerializationError::BufferTooSmall);
        }
        buffer[..16].copy_from_slice(&self.device_id);
        buffer[16..16 + KEY_SIZE].copy_from_slice(&self.key);
        encode_fixed_string(
            &self.custom_name,
            &mut buffer[16 + KEY_SIZE..CONTACT_RECORD_BYTES - 1],
        );
        buffer[CONTACT_RECORD_BYTES - 1] = u8::from(self.blocked);
        Ok(CONTACT_RECORD_BYTES)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        if buffer.len() < CONTACT_RECORD_BYTES {
            return Err(SerializationError::BufferTooSmall);
        }
        let device_id = buffer[..16]
            .try_into()
            .map_err(|_| SerializationError::InvalidFormat)?;
        let key = buffer[16..16 + KEY_SIZE]
            .try_into()
            .map_err(|_| SerializationError::InvalidFormat)?;
        let custom_name = decode_fixed_string(&buffer[16 + KEY_SIZE..CONTACT_RECORD_BYTES - 1])?;
        let blocked = buffer[CONTACT_RECORD_BYTES - 1] == 1;
        Ok((
            Self {
                device_id,
                key,
                custom_name,
                blocked,
            },
            CONTACT_RECORD_BYTES,
        ))
    }
}

fn encode_fixed_string<const N: usize>(value: &String<N>, buffer: &mut [u8]) {
    buffer.fill(0);
    buffer[..value.len()].copy_from_slice(value.as_bytes());
}

fn decode_fixed_string<const N: usize>(bytes: &[u8]) -> Result<String<N>, SerializationError> {
    let len = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let decoded =
        core::str::from_utf8(&bytes[..len]).map_err(|_| SerializationError::InvalidFormat)?;
    let mut value = String::<N>::new();
    value
        .push_str(decoded)
        .map_err(|_| SerializationError::InvalidData)?;
    Ok(value)
}
