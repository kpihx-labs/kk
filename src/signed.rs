//! Bounded, domain-separated room records using the existing certificate key.
//! These signatures prove authorship, not global trust, freshness or consensus.

use crate::{Error, Identity, Peer, Result};
use ring::{rand::SystemRandom, signature, signature::KeyPair};
use serde::Serialize;

pub(crate) const MAX_RECORD: usize = 16_384;

// No Debug or private-key export. Profile-key signatures go through preimage().
// MLS uses a distinct signing key authenticated by a profile-signed binding.
struct ProfileSigner(signature::EcdsaKeyPair);
impl ProfileSigner {
    fn new(identity: &Identity) -> Result<Self> {
        let key = signature::EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            identity.key.secret_der(),
            &SystemRandom::new(),
        )
        .map_err(|_| Error::Identity("room signing requires a P-256 PKCS#8 identity"))?;
        if key.public_key().as_ref() != public_key(&identity.peer())? {
            return Err(Error::Authentication);
        }
        Ok(Self(key))
    }
    fn sign_payload(&self, message: &[u8]) -> Result<Vec<u8>> {
        if message.len() > crate::config::ROOM_MAX_WIRE_BYTES {
            return Err(Error::Limit {
                resource: "signature input",
                max: crate::config::ROOM_MAX_WIRE_BYTES,
            });
        }
        self.0
            .sign(&SystemRandom::new(), message)
            .map(|value| value.as_ref().to_vec())
            .map_err(|_| Error::Authentication)
    }
}

pub(crate) fn preimage(domain: &'static [u8], body: &impl Serialize) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(body).map_err(|_| Error::Protocol)?;
    if body.len() > MAX_RECORD {
        return Err(Error::Limit {
            resource: "signed record",
            max: MAX_RECORD,
        });
    }
    let mut message = Vec::with_capacity(domain.len() + 4 + body.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(&(body.len() as u32).to_be_bytes());
    message.extend_from_slice(&body);
    Ok(message)
}

pub(crate) fn sign(
    identity: &Identity,
    domain: &'static [u8],
    body: &impl Serialize,
) -> Result<Vec<u8>> {
    ProfileSigner::new(identity)?.sign_payload(&preimage(domain, body)?)
}

pub(crate) fn verify(
    peer: &Peer,
    domain: &'static [u8],
    body: &impl Serialize,
    proof: &[u8],
) -> Result<()> {
    if proof.len() > 80 {
        return Err(Error::Authentication);
    }
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, public_key(peer)?)
        .verify(&preimage(domain, body)?, proof)
        .map_err(|_| Error::Authentication)
}

fn public_key(peer: &Peer) -> Result<Vec<u8>> {
    let (remaining, certificate) = x509_parser::parse_x509_certificate(peer.certificate.as_ref())
        .map_err(|_| Error::Authentication)?;
    let public = certificate.public_key();
    let curve = public
        .algorithm
        .parameters
        .as_ref()
        .and_then(|value| value.as_oid().ok());
    if !remaining.is_empty()
        || public.algorithm.algorithm.to_id_string() != "1.2.840.10045.2.1"
        || curve.is_none_or(|value| value.to_id_string() != "1.2.840.10045.3.1.7")
    {
        return Err(Error::Authentication);
    }
    Ok(public.subject_public_key.data.to_vec())
}
