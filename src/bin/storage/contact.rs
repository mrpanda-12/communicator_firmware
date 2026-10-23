use heapless::String;
use sequential_storage::map::{SerializationError, Value};

const CONTACT_RECORD_BYTES: usize = 16 + 64 + 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub device_id: [u8; 16],
    pub password: String<64>,
    pub custom_name: String<32>,
}

impl Contact {
    // Uses the custom label or the device ID as uppercase hexadecimal.
    pub fn display_name(&self) -> String<32> {
        if self.custom_name.is_empty() {
            let mut name = String::<32>::new();
            let digits = b"0123456789ABCDEF";

            for byte in self.device_id {
                let _ = name.push(digits[(byte >> 4) as usize] as char);
                let _ = name.push(digits[(byte & 0x0F) as usize] as char);
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

        // Stores the ID, password, and name in fixed-width fields.
        buffer[..16].copy_from_slice(&self.device_id);
        // Writes password bytes without application-level encryption.
        encode_fixed_string(&self.password, &mut buffer[16..80]);
        encode_fixed_string(&self.custom_name, &mut buffer[80..112]);
        Ok(CONTACT_RECORD_BYTES)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        if buffer.len() < CONTACT_RECORD_BYTES {
            return Err(SerializationError::BufferTooSmall);
        }

        let device_id = buffer[..16]
            .try_into()
            .map_err(|_| SerializationError::InvalidFormat)?;
        let password = decode_fixed_string(&buffer[16..80])?;
        let custom_name = decode_fixed_string(&buffer[80..112])?;

        Ok((
            Self {
                device_id,
                password,
                custom_name,
            },
            CONTACT_RECORD_BYTES,
        ))
    }
}

fn encode_fixed_string<const N: usize>(value: &String<N>, buffer: &mut [u8]) {
    // Pads text with zeros so each field keeps a fixed size.
    buffer.fill(0);
    buffer[..value.len()].copy_from_slice(value.as_bytes());
}

fn decode_fixed_string<const N: usize>(bytes: &[u8]) -> Result<String<N>, SerializationError> {
    // Rejects invalid UTF-8 and text that exceeds the string capacity.
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
