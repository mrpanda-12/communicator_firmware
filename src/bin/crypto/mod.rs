use aes_gcm::{
    Aes256Gcm, Nonce, Tag,
    aead::{AeadInOut, KeyInit},
};
use alloc::vec::Vec;
use esp_hal::rng::Rng;
use sha2::{Digest, Sha256};

pub const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;
pub const OVERHEAD: usize = NONCE_SIZE + TAG_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    InvalidKey,
    EncryptionFailed,
    AuthenticationFailed,
}

pub fn hash(data: &[u8]) -> [u8; KEY_SIZE] {
    Sha256::digest(data).into()
}

pub fn hash_with_salt(data: &[u8], salt: &[u8]) -> [u8; KEY_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(data);
    hasher.finalize().into()
}

pub fn derive_key(password: &[u8]) -> [u8; KEY_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(b"communicator-aes-256-gcm-key-v1");
    hasher.update(password);
    hasher.finalize().into()
}

pub fn derive_pair_key(password: &[u8], my_id: &[u8; 16], peer_id: &[u8; 16]) -> [u8; KEY_SIZE] {
    let mut ids = [*my_id, *peer_id];
    ids.sort_unstable_by(|a, b| a.cmp(b));

    let mut hasher = Sha256::new();
    hasher.update(b"communicator-pair-v1");
    hasher.update(&ids[0]);
    hasher.update(&ids[1]);
    hasher.update(password);
    hasher.finalize().into()
}

pub fn encrypt(
    key: &[u8; KEY_SIZE],
    data: &[u8],
    aad: &[u8],
) -> Result<([u8; NONCE_SIZE], Vec<u8>, [u8; TAG_SIZE]), CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;

    let mut nonce = [0; NONCE_SIZE];
    Rng::new().read(&mut nonce);

    let nonce_ref = Nonce::from(nonce);
    let mut ciphertext = data.to_vec();
    let tag = cipher
        .encrypt_inout_detached(&nonce_ref, aad, ciphertext.as_mut_slice().into())
        .map_err(|_| CryptoError::EncryptionFailed)?;

    Ok((nonce, ciphertext, tag.into()))
}

pub fn decrypt(
    key: &[u8; KEY_SIZE],
    nonce: &[u8; NONCE_SIZE],
    ciphertext: &[u8],
    tag: &[u8; TAG_SIZE],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;

    let nonce = Nonce::from(*nonce);
    let tag = Tag::from(*tag);
    let mut plaintext = ciphertext.to_vec();

    cipher
        .decrypt_inout_detached(&nonce, aad, plaintext.as_mut_slice().into(), &tag)
        .map_err(|_| CryptoError::AuthenticationFailed)?;

    Ok(plaintext)
}
