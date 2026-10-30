use aes_gcm::{
    Aes256Gcm, Nonce, Tag,
    aead::{AeadInOut, KeyInit},
};
use alloc::vec::Vec;
use esp_hal::rng::Rng;
use pbkdf2::pbkdf2_hmac;
use sha2::{Digest, Sha256};

use crate::packet::packet_data::{GCM_NONCE_SIZE, GCM_OVERHEAD, GCM_TAG_SIZE, PacketData};

pub const KEY_SIZE: usize = 32;
/// AEAD sizes are owned by the packet layer, which defines the wire layout.
pub const NONCE_SIZE: usize = GCM_NONCE_SIZE;
pub const TAG_SIZE: usize = GCM_TAG_SIZE;
const PAIR_KDF_ROUNDS: u32 = 100_000;

/// One authenticated ciphertext: the nonce, the ciphertext, and the tag.
pub struct Sealed {
    pub nonce: [u8; NONCE_SIZE],
    pub ciphertext: Vec<u8>,
    pub tag: [u8; TAG_SIZE],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    InvalidKey,
    EncryptionFailed,
    AuthenticationFailed,
}

/// Hash byte runs in order without joining them in a temporary buffer.
pub fn hash_parts(parts: &[&[u8]]) -> [u8; KEY_SIZE] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

pub fn derive_pair_key(password: &[u8], my_id: &[u8; 16], peer_id: &[u8; 16]) -> [u8; KEY_SIZE] {
    // Sort the pair so both peers derive the same key from either direction.
    let mut ids = [*my_id, *peer_id];
    ids.sort_unstable();

    const DOMAIN: &[u8] = b"communicator-pair-v2";
    let mut salt = [0; DOMAIN.len() + 32];
    salt[..DOMAIN.len()].copy_from_slice(DOMAIN);
    salt[DOMAIN.len()..DOMAIN.len() + 16].copy_from_slice(&ids[0]);
    salt[DOMAIN.len() + 16..].copy_from_slice(&ids[1]);

    let mut key = [0; KEY_SIZE];
    pbkdf2_hmac::<Sha256>(password, &salt, PAIR_KDF_ROUNDS, &mut key);
    key
}

pub fn encrypt(key: &[u8; KEY_SIZE], data: &[u8], aad: &[u8]) -> Result<Sealed, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;

    let mut nonce = [0; NONCE_SIZE];
    Rng::new().read(&mut nonce);

    let nonce_ref = Nonce::from(nonce);
    let mut ciphertext = data.to_vec();
    let tag = cipher
        .encrypt_inout_detached(&nonce_ref, aad, ciphertext.as_mut_slice().into())
        .map_err(|_| CryptoError::EncryptionFailed)?;

    Ok(Sealed {
        nonce,
        ciphertext,
        tag: tag.into(),
    })
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

pub fn encrypt_packet_payload(
    packet: &mut PacketData,
    key: &[u8; KEY_SIZE],
    plaintext: &[u8],
) -> Result<(), CryptoError> {
    if !packet.is_encrypted {
        return Err(CryptoError::EncryptionFailed);
    }
    let aad = packet.authenticated_header();
    let Sealed {
        nonce,
        ciphertext,
        tag,
    } = encrypt(key, plaintext, &aad)?;
    packet.payload.clear();
    packet
        .payload
        .extend_from_slice(&nonce)
        .map_err(|_| CryptoError::EncryptionFailed)?;
    packet
        .payload
        .extend_from_slice(&ciphertext)
        .map_err(|_| CryptoError::EncryptionFailed)?;
    packet
        .payload
        .extend_from_slice(&tag)
        .map_err(|_| CryptoError::EncryptionFailed)?;
    Ok(())
}

pub fn decrypt_packet_payload(
    packet: &PacketData,
    key: &[u8; KEY_SIZE],
) -> Result<Vec<u8>, CryptoError> {
    if !packet.is_encrypted || packet.payload.len() < GCM_OVERHEAD {
        return Err(CryptoError::AuthenticationFailed);
    }
    let payload = packet.payload.as_slice();
    let nonce = <[u8; NONCE_SIZE]>::try_from(&payload[..NONCE_SIZE])
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    let tag = <[u8; TAG_SIZE]>::try_from(&payload[payload.len() - TAG_SIZE..])
        .map_err(|_| CryptoError::AuthenticationFailed)?;
    let ciphertext = &payload[NONCE_SIZE..payload.len() - TAG_SIZE];
    decrypt(
        key,
        &nonce,
        ciphertext,
        &tag,
        &packet.authenticated_header(),
    )
}
