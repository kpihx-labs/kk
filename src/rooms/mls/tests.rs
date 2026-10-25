use super::super::RootBundle;
use super::*;

fn fixture() -> (tempfile::TempDir, Vec<Identity>, Anchor, Vec<CryptoMember>) {
    let directory = tempfile::tempdir().unwrap();
    let identities: Vec<_> = (0..3)
        .map(|i| Identity::create(&directory.path().join(format!("mls-{i}"))).unwrap())
        .collect();
    let anchor = RootBundle::create(&identities[0], "MLS synthetic fixture")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let bob = Grant::issue(
        &anchor,
        &[],
        &identities[0],
        &identities[1].peer(),
        Rights::MODERATOR,
    )
    .unwrap();
    let charlie = Grant::issue(
        &anchor,
        std::slice::from_ref(&bob),
        &identities[1],
        &identities[2].peer(),
        Rights::PARTICIPANT,
    )
    .unwrap();
    let actors = vec![
        CryptoMember::new(&identities[0], &anchor, vec![]).unwrap(),
        CryptoMember::new(&identities[1], &anchor, vec![bob.clone()]).unwrap(),
        CryptoMember::new(&identities[2], &anchor, vec![bob, charlie]).unwrap(),
    ];
    (directory, identities, anchor, actors)
}

#[test]
fn mls_packages_bind_certificate_key_room_and_delegated_authority() {
    let (_directory, identities, _anchor, mut actors) = fixture();
    for index in 0..actors.len() {
        let bytes = actors[index].key_package().unwrap();
        let candidate = actors[0].validate_package(&bytes).unwrap();
        assert_eq!(
            candidate.peer().fingerprint(),
            identities[index].peer().fingerprint()
        );
        assert_eq!(candidate.rights().can_invite(), index != 2);
        let mut modified = bytes.clone();
        let last = modified.len() - 1;
        modified[last] ^= 1;
        assert!(actors[0].validate_package(&modified).is_err());
    }
}

#[test]
fn a_valid_mls_signature_is_not_a_certificate_identity_proof() {
    let (_directory, _identities, _anchor, actors) = fixture();
    let signer = SignatureKeyPair::new(SUITE.signature_algorithm()).unwrap();
    let mut claimed = actors[1].credential.clone();
    claimed.signature_key = signer.to_public_vec().into();
    // MLS signatures are valid for the impostor's key, while the opaque Basic
    // credential claims Bob's real certificate. Application binding must reject it.
    let provider = Provider::memory().unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let bundle = KeyPackage::builder()
        .key_package_lifetime(Lifetime::init(now - 60, now + 600))
        .build(SUITE, &provider, &signer, claimed)
        .unwrap();
    let wire = bundle.key_package().tls_serialize_detached().unwrap();
    let raw = KeyPackageIn::tls_deserialize_exact_bytes(&wire).unwrap();
    assert!(
        raw.validate(provider.crypto(), ProtocolVersion::Mls10)
            .is_ok()
    );
    assert!(actors[0].validate_package(&wire).is_err());
}

#[test]
fn package_budgets_lifetimes_frames_and_cross_room_replay_are_bounded() {
    let (_directory, identities, _anchor, mut actors) = fixture();
    let mut hashes = std::collections::BTreeSet::new();
    for _ in 0..config::ROOM_KEY_PACKAGE_BUDGET {
        let wire = actors[1].key_package().unwrap();
        let candidate = actors[0].validate_package(&wire).unwrap();
        assert!(hashes.insert(candidate.hash_reference(&actors[0]).unwrap()));
    }
    assert!(actors[1].key_package().is_err());
    assert!(
        actors[0]
            .validate_package(&vec![0; config::ROOM_MAX_WIRE_BYTES + 1])
            .is_err()
    );
    assert!(actors[0].validate_package(&[255; 32]).is_err());
    let provider = Provider::memory().unwrap();
    let too_long = KeyPackage::builder()
        .key_package_lifetime(Lifetime::new(86_400))
        .build(
            SUITE,
            &provider,
            &actors[0].signer,
            actors[0].credential.clone(),
        )
        .unwrap();
    assert!(
        actors[0]
            .validate_package(&too_long.key_package().tls_serialize_detached().unwrap())
            .is_err()
    );
    let other = RootBundle::create(&identities[0], "Another room")
        .unwrap()
        .verify(&identities[0].peer())
        .unwrap();
    let other_actor = CryptoMember::new(&identities[0], &other, vec![]).unwrap();
    let wire = actors[2].key_package().unwrap();
    assert!(other_actor.validate_package(&wire).is_err());
}
