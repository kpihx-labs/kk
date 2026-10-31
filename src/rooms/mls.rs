//! RFC 9420 key packages bound to the exact K&K certificate and room authority.
//! Package verification is NOT live admission, revocation or ordered delivery.

use super::store::Provider;
use super::{Anchor, Grant, MAX_BUNDLE, Member, Rights};
use crate::{Error, Identity, Peer, Result, config, signed};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_traits::OpenMlsProvider;
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::signatures::Signer;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tls_codec::{DeserializeBytes as TlsDeserializeBytes, Serialize as TlsSerialize};

pub(super) const SUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
const BINDING_DOMAIN: &[u8] = b"K&K MLS identity binding v1\0";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    version: u8,
    room: [u8; 32],
    subject: String,
    grant: [u8; 32],
    mls_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberIdentity {
    certificate: Vec<u8>,
    authority: Vec<Grant>,
    binding: Binding,
    proof: Vec<u8>,
}

/// Private candidate state: holds init private keys for later Welcome processing.
/// It deliberately has no Debug, serialization or private-key export API.
pub struct CryptoMember {
    pub(crate) provider: Provider,
    pub(crate) signer: SignatureKeyPair,
    pub(crate) credential: CredentialWithKey,
    pub(crate) anchor: Anchor,
    packages: usize,
    pub(crate) owner: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientRecord {
    version: u8,
    room: [u8; 32],
    owner: String,
    public_identity: Vec<u8>,
    public_signing_key: Vec<u8>,
    packages: usize,
}

/// Valid cryptographic material only; the current room state must still authorize
/// admission and consume each package once through ordered membership processing.
pub struct Candidate {
    pub(super) package: KeyPackage,
    pub(super) member: Member,
}

/// Only public credential material. Verification still establishes a ceiling,
/// not current membership or a quorum-approved snapshot.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicIdentity {
    credential: Vec<u8>,
    signing_key: Vec<u8>,
}
impl PublicIdentity {
    pub(super) fn from_credential(credential: &Credential, key: &[u8]) -> Result<Self> {
        let basic =
            BasicCredential::try_from(credential.clone()).map_err(|_| Error::Authentication)?;
        if basic.identity().len() > MAX_BUNDLE || key.len() != 32 {
            return Err(Error::Protocol);
        }
        Ok(Self {
            credential: basic.identity().to_vec(),
            signing_key: key.to_vec(),
        })
    }
    pub fn verify(&self, anchor: &Anchor) -> Result<Member> {
        if self.credential.len() > MAX_BUNDLE || self.signing_key.len() != 32 {
            return Err(Error::Protocol);
        }
        verify_credential(
            anchor,
            &BasicCredential::new(self.credential.clone()).into(),
            &self.signing_key,
        )
    }
    pub(crate) fn authority_chain(&self, anchor: &Anchor) -> Result<Vec<Grant>> {
        self.verify(anchor)?;
        let identity: MemberIdentity = super::decode(&self.credential)?;
        Ok(identity.authority)
    }
}
impl Candidate {
    pub fn peer(&self) -> Peer {
        self.member.peer()
    }
    /// Offline authority ceiling, not current room admission or live permissions.
    pub fn rights(&self) -> Rights {
        self.member.rights()
    }
    pub fn public_identity(&self) -> Result<PublicIdentity> {
        let leaf = self.package.leaf_node();
        PublicIdentity::from_credential(leaf.credential(), leaf.signature_key().as_slice())
    }
    pub fn hash_reference(&self, verifier: &CryptoMember) -> Result<Vec<u8>> {
        self.package
            .hash_ref(verifier.provider.crypto())
            .map(|value| value.as_slice().to_vec())
            .map_err(|_| Error::Authentication)
    }
}

impl CryptoMember {
    pub fn new(identity: &Identity, anchor: &Anchor, authority: Vec<Grant>) -> Result<Self> {
        Self::with_provider(identity, anchor, authority, Provider::memory()?)
    }

    pub fn create_private(
        identity: &Identity,
        anchor: &Anchor,
        authority: Vec<Grant>,
        directory: &Path,
    ) -> Result<Self> {
        Self::with_provider(identity, anchor, authority, Provider::create(directory)?)
    }

    fn with_provider(
        identity: &Identity,
        anchor: &Anchor,
        authority: Vec<Grant>,
        provider: Provider,
    ) -> Result<Self> {
        let member = anchor.verify_chain(&authority)?;
        if member.peer().fingerprint() != identity.peer().fingerprint() {
            return Err(Error::Authentication);
        }
        let signer = SignatureKeyPair::new(SUITE.signature_algorithm())
            .map_err(|_| Error::Authentication)?;
        let binding = Binding {
            version: 1,
            room: anchor.id(),
            subject: identity.peer().fingerprint(),
            grant: member.grant_id(),
            mls_key: signer.to_public_vec(),
        };
        let proof = signed::sign(identity, BINDING_DOMAIN, &binding)?;
        let presented = MemberIdentity {
            certificate: identity.peer().certificate.to_vec(),
            authority,
            binding,
            proof,
        };
        let bytes = serde_json::to_vec(&presented).map_err(|_| Error::Protocol)?;
        if bytes.len() > MAX_BUNDLE {
            return Err(Error::Limit {
                resource: "MLS credential",
                max: MAX_BUNDLE,
            });
        }
        let credential = CredentialWithKey {
            credential: BasicCredential::new(bytes).into(),
            signature_key: signer.to_public_vec().into(),
        };
        let actor = Self {
            provider,
            signer,
            credential,
            anchor: anchor.clone(),
            packages: 0,
            owner: identity.peer().fingerprint(),
        };
        let transaction = actor.provider.transaction()?;
        actor
            .signer
            .store(actor.provider.storage())
            .map_err(|_| Error::Storage("cannot save MLS signer"))?;
        actor.save_client()?;
        actor.provider.set_metadata("membership_started", &false)?;
        transaction.commit()?;
        Ok(actor)
    }

    pub fn open_private(identity: &Identity, anchor: &Anchor, directory: &Path) -> Result<Self> {
        let provider = Provider::open(directory)?;
        let record = provider
            .metadata::<ClientRecord>("client")?
            .ok_or(Error::Storage(
                "MLS client state is missing; explicit recovery required",
            ))?;
        if record.version != 1
            || record.room != anchor.id()
            || record.owner != identity.peer().fingerprint()
            || record.packages > config::ROOM_KEY_PACKAGE_BUDGET
            || record.public_identity.len() > MAX_BUNDLE
            || record.public_signing_key.len() != 32
        {
            return Err(Error::Authentication);
        }
        let credential = CredentialWithKey {
            credential: BasicCredential::new(record.public_identity).into(),
            signature_key: record.public_signing_key.clone().into(),
        };
        let member = verify_credential(
            anchor,
            &credential.credential,
            credential.signature_key.as_slice(),
        )?;
        if member.peer().fingerprint() != record.owner {
            return Err(Error::Authentication);
        }
        let signer = SignatureKeyPair::read(
            provider.storage(),
            &record.public_signing_key,
            SUITE.signature_algorithm(),
        )
        .ok_or(Error::Storage("MLS private signer is missing or corrupt"))?;
        let probe = b"K&K restored MLS signer check v1";
        let proof = signer.sign(probe).map_err(|_| Error::Authentication)?;
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::ED25519,
            &record.public_signing_key,
        )
        .verify(probe, &proof)
        .map_err(|_| Error::Authentication)?;
        Ok(Self {
            provider,
            signer,
            credential,
            anchor: anchor.clone(),
            packages: record.packages,
            owner: record.owner,
        })
    }

    fn save_client(&self) -> Result<()> {
        let basic = BasicCredential::try_from(self.credential.credential.clone())
            .map_err(|_| Error::Authentication)?;
        self.provider.set_metadata(
            "client",
            &ClientRecord {
                version: 1,
                room: self.anchor.id(),
                owner: self.owner.clone(),
                public_identity: basic.identity().to_vec(),
                public_signing_key: self.signer.to_public_vec(),
                packages: self.packages,
            },
        )
    }

    /// Export only the public MLS KeyPackage, never KeyPackageBundle/init secrets.
    pub fn key_package(&mut self) -> Result<Vec<u8>> {
        let previous = self.packages;
        let transaction = self.provider.transaction()?;
        let outcome = self.build_package().and_then(|wire| {
            self.save_client()?;
            Ok(wire)
        });
        match outcome {
            Ok(wire) => {
                transaction.commit()?;
                Ok(wire)
            }
            Err(error) => {
                self.packages = previous;
                drop(transaction);
                Err(error)
            }
        }
    }

    fn build_package(&mut self) -> Result<Vec<u8>> {
        if self.packages >= config::ROOM_KEY_PACKAGE_BUDGET {
            return Err(Error::Configuration("key package budget exhausted"));
        }
        self.packages += 1;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Configuration("clock predates UNIX epoch"))?
            .as_secs();
        let end = now
            .checked_add(config::ROOM_KEY_PACKAGE_LIFETIME)
            .ok_or(Error::Configuration("clock overflow"))?;
        let bundle = KeyPackage::builder()
            .key_package_lifetime(Lifetime::init(
                now.saturating_sub(config::ROOM_CLOCK_MARGIN),
                end,
            ))
            .build(SUITE, &self.provider, &self.signer, self.credential.clone())
            .map_err(|_| Error::Authentication)?;
        let bytes = bundle
            .key_package()
            .tls_serialize_detached()
            .map_err(|_| Error::Protocol)?;
        if bytes.len() > config::ROOM_MAX_WIRE_BYTES {
            return Err(Error::Limit {
                resource: "MLS frame",
                max: config::ROOM_MAX_WIRE_BYTES,
            });
        }
        Ok(bytes)
    }

    pub fn validate_package(&self, bytes: &[u8]) -> Result<Candidate> {
        validate_package(&self.anchor, self.provider.crypto(), bytes)
    }
    pub fn public_identity(&self) -> Result<PublicIdentity> {
        PublicIdentity::from_credential(
            &self.credential.credential,
            self.credential.signature_key.as_slice(),
        )
    }
}

pub(super) fn validate_package(
    anchor: &Anchor,
    crypto: &impl OpenMlsCrypto,
    bytes: &[u8],
) -> Result<Candidate> {
    validate_package_inner(anchor, crypto, bytes, false)
}

/// Only for accepted control-log replay, after current freshness was checked at
/// the leader's guarded admission boundary. Never prepares a new MLS admission.
pub(super) fn validate_package_for_replay(
    anchor: &Anchor,
    crypto: &impl OpenMlsCrypto,
    bytes: &[u8],
) -> Result<Candidate> {
    validate_package_inner(anchor, crypto, bytes, true)
}

fn validate_package_inner(
    anchor: &Anchor,
    crypto: &impl OpenMlsCrypto,
    bytes: &[u8],
    committed: bool,
) -> Result<Candidate> {
    if bytes.len() > config::ROOM_MAX_WIRE_BYTES {
        return Err(Error::Limit {
            resource: "MLS frame",
            max: config::ROOM_MAX_WIRE_BYTES,
        });
    }
    let incoming = KeyPackageIn::tls_deserialize_exact_bytes(bytes).map_err(|_| Error::Protocol)?;
    let package = if committed {
        incoming.validate_for_replay(crypto, ProtocolVersion::Mls10)
    } else {
        incoming.validate(crypto, ProtocolVersion::Mls10)
    }
    .map_err(|_| Error::Authentication)?;
    let lifetime = package.life_time();
    if package.ciphersuite() != SUITE
        || package.last_resort()
        || lifetime
            .not_after()
            .checked_sub(lifetime.not_before())
            .is_none_or(|span| span > config::ROOM_KEY_PACKAGE_LIFETIME + config::ROOM_CLOCK_MARGIN)
    {
        return Err(Error::Authentication);
    }
    let leaf = package.leaf_node();
    let member = verify_credential(anchor, leaf.credential(), leaf.signature_key().as_slice())?;
    Ok(Candidate { package, member })
}

pub(super) fn verify_credential(
    anchor: &Anchor,
    credential: &Credential,
    key: &[u8],
) -> Result<Member> {
    let basic = BasicCredential::try_from(credential.clone()).map_err(|_| Error::Authentication)?;
    if basic.identity().len() > MAX_BUNDLE {
        return Err(Error::Limit {
            resource: "MLS credential",
            max: MAX_BUNDLE,
        });
    }
    let presented: MemberIdentity =
        serde_json::from_slice(basic.identity()).map_err(|_| Error::Protocol)?;
    if presented.binding.version != 1
        || presented.binding.room != anchor.id()
        || presented.binding.mls_key.len() != 32
        || presented.binding.mls_key != key
    {
        return Err(Error::Authentication);
    }
    let peer = Peer::from_der(presented.certificate)?;
    if presented.binding.subject != peer.fingerprint() {
        return Err(Error::Authentication);
    }
    signed::verify(&peer, BINDING_DOMAIN, &presented.binding, &presented.proof)?;
    let member = anchor.verify_chain(&presented.authority)?;
    if member.peer().fingerprint() != peer.fingerprint()
        || member.grant_id() != presented.binding.grant
    {
        return Err(Error::Authentication);
    }
    Ok(member)
}

#[cfg(all(test, unix))]
mod tests;
