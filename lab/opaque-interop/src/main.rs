//! Test-only independent-process interop. Never prints keys or private setup.
#![forbid(unsafe_code)]

use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialRequest,
    CredentialResponse, Identifiers, RegistrationRequest, RegistrationResponse, RegistrationUpload,
    ServerLogin, ServerLoginParameters, ServerRegistration, ServerSetup,
    argon2::{Algorithm, Argon2, Params, Version},
};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256, Sha512};
use std::io::{self, Read, Write};

struct Suite;
impl CipherSuite for Suite {
    type OprfCs = opaque_ke::Ristretto255;
    type KeyExchange = opaque_ke::TripleDh<opaque_ke::Ristretto255, Sha512>;
    type Ksf = Argon2<'static>;
}

fn receive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut header = [0; 4];
    io::stdin().read_exact(&mut header)?;
    let size = u32::from_be_bytes(header) as usize;
    if size > 16_384 {
        return Err("oversized test frame".into());
    }
    let mut bytes = vec![0; size];
    io::stdin().read_exact(&mut bytes)?;
    Ok(bytes)
}

fn send(bytes: &[u8]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(&(bytes.len() as u32).to_be_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

fn identifiers() -> Identifiers<'static> {
    Identifiers {
        client: Some(b"interop-client"),
        server: Some(b"interop-server"),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let password: &[u8] = if args.get(2).is_some_and(|s| s == "wrong-password") {
        b"public-wrong-fixture"
    } else {
        b"public-opaque-interop-fixture"
    };
    let context: &[u8] = if args.get(2).is_some_and(|s| s == "wrong-context") {
        b"different-context"
    } else {
        b"opaque-interop-context-v1"
    };
    let ksf = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19_456, 2, 1, None).map_err(|_| "invalid test KSF parameters")?,
    );
    let mut rng = OsRng;
    if args.get(1).is_some_and(|s| s == "server") {
        let setup = ServerSetup::<Suite>::new(&mut rng);
        let response = ServerRegistration::<Suite>::start(
            &setup,
            RegistrationRequest::deserialize(&receive()?)?,
            b"interop-record",
        )?;
        send(&response.message.serialize())?;
        let record = ServerRegistration::finish(RegistrationUpload::deserialize(&receive()?)?);
        let login = ServerLogin::start(
            &mut rng,
            &setup,
            Some(record),
            CredentialRequest::deserialize(&receive()?)?,
            b"interop-record",
            ServerLoginParameters {
                context: Some(context),
                identifiers: identifiers(),
            },
        )?;
        send(&login.message.serialize())?;
        let finished = login.state.finish(
            CredentialFinalization::deserialize(&receive()?)?,
            ServerLoginParameters {
                context: Some(context),
                identifiers: identifiers(),
            },
        )?;
        send(&Sha256::digest(finished.session_key.as_slice()))?;
    } else {
        let registration = ClientRegistration::<Suite>::start(&mut rng, password)?;
        send(&registration.message.serialize())?;
        let uploaded = registration.state.finish(
            &mut rng,
            password,
            RegistrationResponse::deserialize(&receive()?)?,
            ClientRegistrationFinishParameters::new(identifiers(), Some(&ksf)),
        )?;
        send(&uploaded.message.serialize())?;
        let login_password: &[u8] = if args.get(2).is_some_and(|s| s == "wrong-password") {
            b"changed-after-registration"
        } else {
            password
        };
        let login = ClientLogin::<Suite>::start(&mut rng, login_password)?;
        send(&login.message.serialize())?;
        let finished = login.state.finish(
            &mut rng,
            login_password,
            CredentialResponse::deserialize(&receive()?)?,
            ClientLoginFinishParameters::new(Some(context), identifiers(), Some(&ksf)),
        )?;
        send(&finished.message.serialize())?;
        send(&Sha256::digest(finished.session_key.as_slice()))?;
    }
    Ok(())
}

fn main() {
    if run().is_err() {
        eprintln!("OPAQUE test peer refused the exchange");
        std::process::exit(1);
    }
}
