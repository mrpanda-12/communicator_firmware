use alloc::vec::Vec;
use sha2::{Digest, Sha256};

pub fn from_with_size(data: &[u8], size: u8) -> Result<Vec<u8>, &'static str> {
    if size > 32 {
        return Err("SHA-256 output cannot exceed 32 bytes");
    }

    let hash = Sha256::digest(data);

    Ok(hash[..usize::from(size)].to_vec())
}
