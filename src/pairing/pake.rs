//! Single-use OPAQUE ceremony state. Registration is local, never a network upload.

use crate::{Error, Result};
use age::secrecy::{ExposeSecret, SecretString};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialRequest,
    CredentialResponse, Identifiers, ServerLogin, ServerLoginParameters, ServerRegistration,
    ServerSetup,
    argon2::{Algorithm, Argon2, Params, Version},
    rand::{RngCore, rngs::OsRng},
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

struct Suite;
impl CipherSuite for Suite {
    type OprfCs = opaque_ke::Ristretto255;
    type KeyExchange = opaque_ke::TripleDh<opaque_ke::Ristretto255, sha2_10::Sha512>;
    type Ksf = Argon2<'static>;
}

const DOMAIN: &[u8] = b"kk-pair/1;ristretto255;argon2id-m19456-t2-p1";
const MAX_FRAME: usize = 16_384;

/// Bootstrap purpose, not application membership or a global contact grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Contact,
    Room([u8; 32]),
}

fn ksf() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19_456, 2, 1, None).expect("fixed Argon2 parameters"),
    )
}
fn identifiers(id: &[u8; 16]) -> Identifiers<'_> {
    Identifiers {
        client: Some(b"kk-pair-client"),
        server: Some(id),
    }
}
fn context(id: &[u8; 16], scope: Scope) -> Vec<u8> {
    match scope {
        Scope::Contact => [DOMAIN, id.as_slice()].concat(),
        Scope::Room(room) => [DOMAIN, b"\0room\0", room.as_slice(), id.as_slice()].concat(),
    }
}
fn transcript(
    id: &[u8; 16],
    scope: Scope,
    request: &[u8],
    response: &[u8],
    finish: &[u8],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(context(id, scope));
    for message in [request, response, finish] {
        hash.update((message.len() as u32).to_be_bytes());
        hash.update(message);
    }
    hash.finalize().into()
}

pub struct Invitation {
    scope: Scope,
    id: [u8; 16],
    code: Option<SecretString>,
    setup: ServerSetup<Suite>,
    record: Option<ServerRegistration<Suite>>,
    created: Instant,
    ttl: Duration,
}
pub struct ServerExchange {
    scope: Scope,
    state: ServerLogin<Suite>,
    id: [u8; 16],
    request: Vec<u8>,
    response: Vec<u8>,
}
pub struct ClientExchange {
    scope: Scope,
    state: ClientLogin<Suite>,
    code: SecretString,
    id: [u8; 16],
    request: Vec<u8>,
}

impl Invitation {
    pub fn generate(ttl: Duration) -> Result<Self> {
        Self::generate_scoped(ttl, Scope::Contact)
    }
    pub fn generate_scoped(ttl: Duration, scope: Scope) -> Result<Self> {
        if ttl.is_zero() || ttl > Duration::from_secs(300) {
            return Err(Error::Pairing);
        }
        let mut rng = OsRng;
        let mut id = [0; 16];
        rng.fill_bytes(&mut id);
        let mut entropy = Zeroizing::new([0; 12]);
        rng.fill_bytes(entropy.as_mut());
        let words = bip39::Language::English.word_list();
        let code = SecretString::from(
            entropy
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| words[(u16::from_be_bytes([bytes[0], bytes[1]]) & 2047) as usize])
                .collect::<Vec<_>>()
                .join("-"),
        );
        let setup = ServerSetup::<Suite>::new(&mut rng);
        let client = ClientRegistration::<Suite>::start(&mut rng, code.expose_secret().as_bytes())
            .map_err(|_| Error::Pairing)?;
        let response = ServerRegistration::<Suite>::start(&setup, client.message, &id)
            .map_err(|_| Error::Pairing)?;
        let stretch = ksf();
        let upload = client
            .state
            .finish(
                &mut rng,
                code.expose_secret().as_bytes(),
                response.message,
                ClientRegistrationFinishParameters::new(identifiers(&id), Some(&stretch)),
            )
            .map_err(|_| Error::Pairing)?;
        Ok(Self {
            scope,
            id,
            code: Some(code),
            setup,
            record: Some(ServerRegistration::finish(upload.message)),
            created: Instant::now(),
            ttl,
        })
    }
    pub fn id(&self) -> [u8; 16] {
        self.id
    }
    pub fn scope(&self) -> Scope {
        self.scope
    }
    pub fn remaining(&self) -> Result<Duration> {
        self.ttl
            .checked_sub(self.created.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(Error::Pairing)
    }
    pub fn code(&self) -> Result<&SecretString> {
        self.code.as_ref().ok_or(Error::Pairing)
    }
    pub fn start(&mut self, request: &[u8]) -> Result<(ServerExchange, Vec<u8>)> {
        // Consume before parsing: malformed or racing attempts do not get free guesses.
        let record = self.record.take().ok_or(Error::Pairing)?;
        self.code.take();
        if self.created.elapsed() >= self.ttl || request.len() > MAX_FRAME {
            return Err(Error::Pairing);
        }
        let binding = context(&self.id, self.scope);
        let started = ServerLogin::start(
            &mut OsRng,
            &self.setup,
            Some(record),
            CredentialRequest::deserialize(request).map_err(|_| Error::Pairing)?,
            &self.id,
            ServerLoginParameters {
                context: Some(&binding),
                identifiers: identifiers(&self.id),
            },
        )
        .map_err(|_| Error::Pairing)?;
        let response = started.message.serialize().to_vec();
        Ok((
            ServerExchange {
                scope: self.scope,
                state: started.state,
                id: self.id,
                request: request.to_vec(),
                response: response.clone(),
            },
            response,
        ))
    }
}

impl ClientExchange {
    pub fn start(id: [u8; 16], code: SecretString) -> Result<(Self, Vec<u8>)> {
        Self::start_scoped(id, code, Scope::Contact)
    }
    pub fn start_scoped(id: [u8; 16], code: SecretString, scope: Scope) -> Result<(Self, Vec<u8>)> {
        if code.expose_secret().len() > 512 || code.expose_secret().is_empty() {
            return Err(Error::Pairing);
        }
        let started = ClientLogin::<Suite>::start(&mut OsRng, code.expose_secret().as_bytes())
            .map_err(|_| Error::Pairing)?;
        let request = started.message.serialize().to_vec();
        Ok((
            Self {
                scope,
                state: started.state,
                code,
                id,
                request: request.clone(),
            },
            request,
        ))
    }
    pub fn finish(self, response: &[u8]) -> Result<(Channel, Vec<u8>)> {
        if response.len() > MAX_FRAME {
            return Err(Error::Pairing);
        }
        let binding = context(&self.id, self.scope);
        let stretch = ksf();
        let finished = self
            .state
            .finish(
                &mut OsRng,
                self.code.expose_secret().as_bytes(),
                CredentialResponse::deserialize(response).map_err(|_| Error::Pairing)?,
                ClientLoginFinishParameters::new(
                    Some(&binding),
                    identifiers(&self.id),
                    Some(&stretch),
                ),
            )
            .map_err(|_| Error::Pairing)?;
        let finalization = finished.message.serialize().to_vec();
        let hash = transcript(&self.id, self.scope, &self.request, response, &finalization);
        Ok((
            Channel::new(finished.session_key.as_slice(), hash, false)?,
            finalization,
        ))
    }
}

impl ServerExchange {
    pub fn finish(self, finalization: &[u8]) -> Result<Channel> {
        if finalization.len() > MAX_FRAME {
            return Err(Error::Pairing);
        }
        let binding = context(&self.id, self.scope);
        let finished = self
            .state
            .finish(
                CredentialFinalization::deserialize(finalization).map_err(|_| Error::Pairing)?,
                ServerLoginParameters {
                    context: Some(&binding),
                    identifiers: identifiers(&self.id),
                },
            )
            .map_err(|_| Error::Pairing)?;
        Channel::new(
            finished.session_key.as_slice(),
            transcript(
                &self.id,
                self.scope,
                &self.request,
                &self.response,
                finalization,
            ),
            true,
        )
    }
}

/// Bootstrap metadata only; directional keys and monotonic nonces are not cloned.
pub struct Channel {
    send: Zeroizing<[u8; 32]>,
    receive: Zeroizing<[u8; 32]>,
    transcript: [u8; 32],
    sent: u64,
    received: u64,
}
impl Channel {
    fn new(key: &[u8], transcript: [u8; 32], server: bool) -> Result<Self> {
        let kdf = Hkdf::<sha2_10::Sha256>::new(Some(&transcript), key);
        let mut client_key = Zeroizing::new([0; 32]);
        let mut server_key = Zeroizing::new([0; 32]);
        kdf.expand(b"kk-pair/1 client-to-server", client_key.as_mut())
            .map_err(|_| Error::Pairing)?;
        kdf.expand(b"kk-pair/1 server-to-client", server_key.as_mut())
            .map_err(|_| Error::Pairing)?;
        let (send, receive) = if server {
            (server_key, client_key)
        } else {
            (client_key, server_key)
        };
        Ok(Self {
            send,
            receive,
            transcript,
            sent: 0,
            received: 0,
        })
    }
    pub fn seal(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if bytes.len() > MAX_FRAME || self.sent >= 8 {
            return Err(Error::Pairing);
        }
        let mut nonce = [0; 12];
        nonce[4..].copy_from_slice(&self.sent.to_be_bytes());
        self.sent += 1;
        ChaCha20Poly1305::new_from_slice(self.send.as_ref())
            .map_err(|_| Error::Pairing)?
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: bytes,
                    aad: &self.transcript,
                },
            )
            .map_err(|_| Error::Pairing)
    }
    pub fn open(&mut self, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if bytes.len() > MAX_FRAME + 16 || self.received >= 8 {
            return Err(Error::Pairing);
        }
        let mut nonce = [0; 12];
        nonce[4..].copy_from_slice(&self.received.to_be_bytes());
        self.received += 1;
        let bytes = ChaCha20Poly1305::new_from_slice(self.receive.as_ref())
            .map_err(|_| Error::Pairing)?
            .decrypt(
                (&nonce).into(),
                Payload {
                    msg: bytes,
                    aad: &self.transcript,
                },
            )
            .map_err(|_| Error::Pairing)?;
        Ok(Zeroizing::new(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_is_in_opaque_and_channel_transcripts_not_just_an_address_hint() {
        let scope = Scope::Room([1; 32]);
        let mut invitation = Invitation::generate_scoped(Duration::from_secs(5), scope).unwrap();
        let (client, request) = ClientExchange::start_scoped(
            invitation.id(),
            invitation.code().unwrap().clone(),
            scope,
        )
        .unwrap();
        let (server, response) = invitation.start(&request).unwrap();
        let (mut client, finalization) = client.finish(&response).unwrap();
        let mut server = server.finish(&finalization).unwrap();
        assert_eq!(
            server
                .open(&client.seal(b"scoped metadata").unwrap())
                .unwrap()
                .as_slice(),
            b"scoped metadata"
        );
        for wrong in [Scope::Contact, Scope::Room([2; 32])] {
            let mut invitation =
                Invitation::generate_scoped(Duration::from_secs(5), scope).unwrap();
            let (client, request) = ClientExchange::start_scoped(
                invitation.id(),
                invitation.code().unwrap().clone(),
                wrong,
            )
            .unwrap();
            let (_, response) = invitation.start(&request).unwrap();
            assert!(client.finish(&response).is_err());
            assert!(invitation.start(&request).is_err()); // Still ONE global attempt.
        }
    }
    #[test]
    fn correct_code_confirms_both_directions_and_replay_fails() {
        let mut invitation = Invitation::generate(Duration::from_secs(60)).unwrap();
        let (client, request) =
            ClientExchange::start(invitation.id(), invitation.code().unwrap().clone()).unwrap();
        let (server, response) = invitation.start(&request).unwrap();
        assert!(invitation.start(&request).is_err());
        let (mut client, finalization) = client.finish(&response).unwrap();
        let mut server = server.finish(&finalization).unwrap();
        let packet = client
            .seal(b"public certificate and grant fixture")
            .unwrap();
        assert_eq!(
            server.open(&packet).unwrap().as_slice(),
            b"public certificate and grant fixture"
        );
        assert!(server.open(&packet).is_err());
        assert_eq!(
            client
                .open(&server.seal(b"confirmed").unwrap())
                .unwrap()
                .as_slice(),
            b"confirmed"
        );
    }
    #[test]
    fn wrong_codes_and_malformed_first_attempts_consume_the_invitation() {
        let mut invitation = Invitation::generate(Duration::from_secs(60)).unwrap();
        let (client, request) = ClientExchange::start(
            invitation.id(),
            SecretString::from("wrong synthetic code".to_owned()),
        )
        .unwrap();
        let (_, response) = invitation.start(&request).unwrap();
        assert!(client.finish(&response).is_err());
        assert!(invitation.start(&request).is_err());
        let mut invitation = Invitation::generate(Duration::from_secs(60)).unwrap();
        assert!(invitation.start(b"invalid").is_err());
        assert!(invitation.start(b"invalid").is_err());
    }

    #[test]
    fn expiration_and_cross_invitation_contexts_fail_closed() {
        let mut invitation = Invitation::generate(Duration::from_secs(1)).unwrap();
        invitation.created -= Duration::from_secs(2);
        let (_, request) =
            ClientExchange::start(invitation.id(), invitation.code().unwrap().clone()).unwrap();
        assert!(invitation.start(&request).is_err());
        assert!(invitation.start(&request).is_err());
        let mut invitation = Invitation::generate(Duration::from_secs(60)).unwrap();
        let mut other = invitation.id();
        other[0] ^= 1;
        let (client, request) =
            ClientExchange::start(other, invitation.code().unwrap().clone()).unwrap();
        let (_, response) = invitation.start(&request).unwrap();
        assert!(client.finish(&response).is_err());
    }
}
