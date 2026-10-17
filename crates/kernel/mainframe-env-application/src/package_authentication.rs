//! Bounded standard-envelope wire adaptation; callers retain MAC and trust authority.

use crate::{InstallProblem, PackageSignature};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use coset::{
    Algorithm, CoseMac0, Header, MacContext, ProtectedHeader, TaggedCborSerializable, iana,
    mac_structure_data,
};

pub const PACKAGE_AUTHENTICATION_ALGORITHM: &str = "cose-mac0-hmac256@1";
pub const LEGACY_PACKAGE_AUTHENTICATION_ALGORITHM: &str = "hmac-sha256@1";
const EXTERNAL_AAD: &[u8] = b"mainframe-env.package-auth@1";
const MAX_ENCODED_BYTES: usize = 256;
const MAX_DECODED_BYTES: usize = 192;

fn valid_inputs(key_id: &str, identity: &str) -> bool {
    !key_id.is_empty()
        && key_id.len() <= 64
        && !key_id.chars().any(char::is_control)
        && identity.len() == 71
        && identity.starts_with("sha256:")
        && identity.as_bytes()[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Encode the current profile with a caller-owned MAC callback. No key is retained or resolved.
pub fn encode_package_authentication(
    key_id: &str,
    identity: &str,
    mac: impl FnOnce(&[u8]) -> [u8; 32],
) -> Result<PackageSignature, InstallProblem> {
    if !valid_inputs(key_id, identity) {
        return Err(InstallProblem::InvalidSignature);
    }
    let mut envelope = CoseMac0 {
        protected: ProtectedHeader {
            original_data: None,
            header: Header {
                alg: Some(Algorithm::Assigned(iana::Algorithm::HMAC_256_256)),
                key_id: key_id.as_bytes().to_vec(),
                ..Header::default()
            },
        },
        payload: Some(identity.as_bytes().to_vec()),
        ..CoseMac0::default()
    };
    let data = mac_structure_data(
        MacContext::CoseMac0,
        envelope.protected.clone(),
        EXTERNAL_AAD,
        identity.as_bytes(),
    );
    envelope.tag = mac(&data).to_vec();
    let bytes = envelope
        .to_tagged_vec()
        .map_err(|_| InstallProblem::InvalidSignature)?;
    if bytes.len() > MAX_DECODED_BYTES {
        return Err(InstallProblem::InvalidSignature);
    }
    let value = STANDARD_NO_PAD.encode(bytes);
    if value.len() > MAX_ENCODED_BYTES {
        return Err(InstallProblem::InvalidSignature);
    }
    Ok(PackageSignature {
        algorithm: PACKAGE_AUTHENTICATION_ALGORITHM.into(),
        key_id: key_id.into(),
        value,
    })
}

/// Verify borrowed owned-signature fields. Structural refusals never call the trust callback.
pub fn verify_package_authentication(
    key_id: &str,
    algorithm: &str,
    identity: &str,
    value: &str,
    verify: impl FnOnce(&[u8], &[u8]) -> bool,
) -> bool {
    if value.len() > MAX_ENCODED_BYTES
        || algorithm != PACKAGE_AUTHENTICATION_ALGORITHM
        || !valid_inputs(key_id, identity)
    {
        return false;
    }
    let mut decoded = [0_u8; MAX_DECODED_BYTES];
    let Ok(count) = STANDARD_NO_PAD.decode_slice(value, &mut decoded) else {
        return false;
    };
    if count > MAX_DECODED_BYTES {
        return false;
    }
    let bytes = &decoded[..count];
    let Ok(envelope) = CoseMac0::from_tagged_slice(bytes) else {
        return false;
    };
    let header = &envelope.protected.header;
    if header.alg != Some(Algorithm::Assigned(iana::Algorithm::HMAC_256_256))
        || header.key_id != key_id.as_bytes()
        || !header.crit.is_empty()
        || header.content_type.is_some()
        || !header.iv.is_empty()
        || !header.partial_iv.is_empty()
        || !header.counter_signatures.is_empty()
        || !header.rest.is_empty()
        || !envelope.unprotected.is_empty()
        || envelope.payload.as_deref() != Some(identity.as_bytes())
        || envelope.tag.len() != 32
    {
        return false;
    }
    // Clearing the clone's original bytes is essential: coset otherwise preserves their encoding.
    // The original envelope below retains those authenticated bytes for MAC_structure.
    let mut canonical = envelope.clone();
    canonical.protected.original_data = None;
    if !canonical
        .to_tagged_vec()
        .is_ok_and(|canonical| canonical == bytes)
    {
        return false;
    }
    envelope
        .verify_payload_tag(
            EXTERNAL_AAD,
            || (),
            |tag, data| if verify(tag, data) { Ok(()) } else { Err(()) },
        )
        .is_ok()
}

#[cfg(test)]
mod tests;
