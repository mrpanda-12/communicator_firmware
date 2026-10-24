use alloc::{format, string::String, vec::Vec};
use core::convert::TryFrom;

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::{Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use sequential_storage::map::Value;

use crate::storage::message::{MessagePage, StoredMessage};

const RECORD_HEADER_BYTES: usize = 4;
const BROADCAST_FILE: &str = "msg_broadcast_v2";

/// Fixed timestamp used until a GPS- or RTC-backed source is connected.
#[derive(Clone, Copy, Debug)]
pub struct FixedTimeSource;

impl TimeSource for FixedTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// Holds the SD card and FAT volume-manager state for SPI-based storage.
pub struct SdCardStorage<SPI, DELAY, TS = FixedTimeSource>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TS: TimeSource,
{
    pub volume_mgr: VolumeManager<SdCard<SPI, DELAY>, TS, 4, 4, 4>,
}

impl<SPI, DELAY, TS> SdCardStorage<SPI, DELAY, TS>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TS: TimeSource,
{
    /// 1/ Wrap an initialized SPI device as a FAT volume manager.
    ///
    /// Card and volume discovery are performed by `embedded-sdmmc` when the
    /// first file operation opens volume zero.
    pub fn new(spi: SPI, delay: DELAY, time_source: TS) -> Self {
        let sdcard = SdCard::new(spi, delay);
        let volume_mgr = VolumeManager::new_with_limits(sdcard, time_source, 4);
        Self { volume_mgr }
    }

    /// 2/ Append new messages to a peer log or, with `None`, the broadcast log.
    ///
    /// The input is serialized one message at a time into the reusable buffer;
    /// callers should pass only newly created messages.
    pub fn append_messages(
        &mut self,
        device_id: Option<&[u8; 16]>,
        messages: &[StoredMessage],
        buffer: &mut [u8],
    ) -> Result<(), String> {
        let filename = message_file_name(device_id);
        if buffer.len() <= RECORD_HEADER_BYTES {
            return Err(String::from("message scratch buffer is too small"));
        }
        let volume = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(|e| format!("SD open_volume failed: {:?}", e))?;
        let dir = volume
            .open_root_dir()
            .map_err(|e| format!("SD open_root_dir failed: {:?}", e))?;
        let file = match dir.open_long_name_file_in_dir(&filename, Mode::ReadWriteCreateOrAppend) {
            Ok(file) => file,
            Err(e) => {
                let _ = self.volume_mgr.close_dir(dir.to_raw_directory());
                return Err(format!("SD open append file failed: {:?}", e));
            }
        };
        let write_result = (|| -> Result<(), String> {
            for message in messages {
                let len = message
                    .serialize_into(&mut buffer[RECORD_HEADER_BYTES..])
                    .map_err(|e| format!("message serialization failed: {:?}", e))?;
                if len == 0 || len > buffer.len() - RECORD_HEADER_BYTES {
                    return Err(String::from(
                        "serialized message does not fit scratch buffer",
                    ));
                }
                let len = u32::try_from(len)
                    .map_err(|_| String::from("serialized message is too large"))?;
                buffer[..RECORD_HEADER_BYTES].copy_from_slice(&len.to_le_bytes());
                file.write(&buffer[..RECORD_HEADER_BYTES + len as usize])
                    .map_err(|e| format!("SD message append failed: {:?}", e))?;
            }
            file.flush()
                .map_err(|e| format!("SD flush failed: {:?}", e))
        })();
        let close_file = file
            .close()
            .map_err(|e| format!("SD close file failed: {:?}", e));
        let close_dir = self
            .volume_mgr
            .close_dir(dir.to_raw_directory())
            .map_err(|e| format!("SD close directory failed: {:?}", e));
        write_result?;
        close_file?;
        close_dir
    }

    /// 3/ Read a bounded page from a peer log or the broadcast log.
    ///
    /// The cursor is a byte offset. Start at zero and continue with the
    /// returned `next_offset`; do not reuse a flash record-index cursor here.
    pub fn load_messages_page(
        &mut self,
        device_id: Option<&[u8; 16]>,
        offset: u32,
        max_messages: usize,
        buffer: &mut [u8],
    ) -> Result<MessagePage, String> {
        if max_messages == 0 || buffer.len() <= RECORD_HEADER_BYTES {
            return Err(String::from("invalid page size or message scratch buffer"));
        }
        let filename = message_file_name(device_id);
        let volume = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(|e| format!("SD open_volume failed: {:?}", e))?;
        let dir = volume
            .open_root_dir()
            .map_err(|e| format!("SD open_root_dir failed: {:?}", e))?;
        let file = match dir.open_long_name_file_in_dir(&filename, Mode::ReadWriteCreateOrAppend) {
            Ok(file) => file,
            Err(e) => {
                let _ = self.volume_mgr.close_dir(dir.to_raw_directory());
                return Err(format!("SD open message file failed: {:?}", e));
            }
        };
        let read_result = (|| -> Result<MessagePage, String> {
            let file_len = file.length();
            if offset > file_len {
                return Err(format!(
                    "message offset {offset} exceeds file length {file_len}"
                ));
            }
            file.seek_from_start(offset)
                .map_err(|e| format!("SD seek failed: {:?}", e))?;
            let mut messages = Vec::new();
            let mut next_offset = offset;
            while messages.len() < max_messages && next_offset < file_len {
                let mut header = [0; RECORD_HEADER_BYTES];
                read_exact(&file, &mut header, "record header")?;
                let payload_len = u32::from_le_bytes(header) as usize;
                if payload_len == 0 || payload_len > buffer.len() - RECORD_HEADER_BYTES {
                    return Err(String::from("invalid message record length"));
                }
                let after = next_offset
                    .checked_add(RECORD_HEADER_BYTES as u32)
                    .and_then(|n| n.checked_add(u32::try_from(payload_len).ok()?))
                    .ok_or_else(|| String::from("message offset overflow"))?;
                if after > file_len {
                    return Err(String::from("truncated message record"));
                }
                read_exact(
                    &file,
                    &mut buffer[RECORD_HEADER_BYTES..RECORD_HEADER_BYTES + payload_len],
                    "message",
                )?;
                let (message, consumed) = StoredMessage::deserialize_from(
                    &buffer[RECORD_HEADER_BYTES..RECORD_HEADER_BYTES + payload_len],
                )
                .map_err(|e| format!("message deserialization failed: {:?}", e))?;
                if consumed != payload_len {
                    return Err(String::from("message record length mismatch"));
                }
                next_offset = after;
                messages.push(message);
            }
            Ok(MessagePage {
                messages,
                next_offset,
                done: next_offset >= file_len,
            })
        })();
        let close_file = file
            .close()
            .map_err(|e| format!("SD close file failed: {:?}", e));
        let close_dir = self
            .volume_mgr
            .close_dir(dir.to_raw_directory())
            .map_err(|e| format!("SD close directory failed: {:?}", e));
        let page = read_result?;
        close_file?;
        close_dir?;
        Ok(page)
    }
}

fn read_exact<SPI, DELAY, TS>(
    file: &embedded_sdmmc::File<'_, embedded_sdmmc::SdCard<SPI, DELAY>, TS, 4, 4, 4>,
    buffer: &mut [u8],
    record: &str,
) -> Result<(), String>
where
    SPI: SpiDevice<u8>,
    DELAY: DelayNs,
    TS: TimeSource,
{
    let mut offset = 0;
    while offset < buffer.len() {
        let read = file
            .read(&mut buffer[offset..])
            .map_err(|e| format!("SD {record} read failed: {:?}", e))?;
        if read == 0 {
            return Err(format!("truncated message {record}"));
        }
        offset += read;
    }
    Ok(())
}

fn message_file_name(device_id: Option<&[u8; 16]>) -> String {
    let Some(device_id) = device_id else {
        return String::from(BROADCAST_FILE);
    };
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut filename = String::from("msgv2_");
    for byte in device_id {
        filename.push(HEX[(byte >> 4) as usize] as char);
        filename.push(HEX[(byte & 0x0f) as usize] as char);
    }
    filename
}
