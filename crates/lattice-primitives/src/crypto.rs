use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use lattice_multikey::{KeyType, Multikey};
use sha2::{Digest, Sha256};

pub const SIGNATURE_DOMAIN: &str = "lattice-tx-v1:";
pub fn signature_payload(message: &str) -> Vec<u8> {
    [SIGNATURE_DOMAIN.as_bytes(), message.as_bytes()].concat()
}
pub fn sha256(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}
// Matches the pinned Swift Data(hex:) pair parser, including +n and -0.
// Direct signature verification separately requires canonical lowercase text.
pub(crate) fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.is_ascii() || !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            let pair = &value[index..index + 2];
            if pair == "-0" {
                Some(0)
            } else {
                u8::from_str_radix(pair, 16).ok()
            }
        })
        .collect()
}
pub fn generate_key_pair() -> (String, String) {
    let key = SigningKey::generate(&mut rand_core::OsRng);
    (
        hex::encode(key.to_bytes()),
        Multikey {
            key_type: KeyType::Ed25519,
            key_bytes: key.verifying_key().to_bytes().to_vec(),
        }
        .hex_encoded(),
    )
}
pub fn sign(message: &str, private_key_hex: &str) -> Option<String> {
    let bytes: [u8; 32] = decode_hex(private_key_hex)?.try_into().ok()?;
    Some(hex::encode(
        SigningKey::from_bytes(&bytes)
            .sign(&signature_payload(message))
            .to_bytes(),
    ))
}
pub fn verify(message: &str, signature: &str, public_key_hex: &str) -> bool {
    let Some(sig) = decode_hex(signature) else {
        return false;
    };
    if hex::encode(&sig) != signature {
        return false;
    }
    let Ok(key) = Multikey::from_hex(public_key_hex) else {
        return false;
    };
    if key.hex_encoded() != public_key_hex || key.key_type != KeyType::Ed25519 {
        return false;
    }
    let Ok(bytes) = <[u8; 32]>::try_from(key.key_bytes.as_slice()) else {
        return false;
    };
    let (Ok(key), Ok(signature)) = (
        VerifyingKey::from_bytes(&bytes),
        Signature::from_slice(&sig),
    ) else {
        return false;
    };
    key.verify(&signature_payload(message), &signature).is_ok()
}
pub fn create_address(public_key: &str) -> Result<String, lattice_cashew::Error> {
    #[derive(serde::Serialize)]
    struct PublicKey {
        key: String,
    }
    let key = Multikey::from_hex(public_key)
        .map(|k| k.hex_encoded())
        .unwrap_or_else(|_| public_key.into());
    crate::content_id(&PublicKey { key })
}
pub fn is_address(address: &str, public_key: &str) -> bool {
    crate::standard_cid(address)
        && create_address(public_key).is_ok_and(|expected| expected == address)
}
