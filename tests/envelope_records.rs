use std::io::{Read, Write};

use age::x25519;
use kk::{Error, config, envelope::record};
use zeroize::{Zeroize, Zeroizing};

fn seal(bytes: &[u8], identity: &x25519::Identity) -> Vec<u8> {
    record::seal(Zeroizing::new(bytes.to_vec()), &identity.to_public()).unwrap()
}

fn standard_seal(bytes: &[u8], recipients: &[x25519::Recipient]) -> Vec<u8> {
    let encryptor =
        age::Encryptor::with_recipients(recipients.iter().map(|key| key as &dyn age::Recipient))
            .unwrap();
    let mut ciphertext = Vec::new();
    let mut writer = encryptor.wrap_output(&mut ciphertext).unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap();
    ciphertext
}

#[test]
fn empty_unicode_and_binary_are_standard_age_records() {
    let identity = x25519::Identity::generate();
    let binary: Vec<u8> = (0..=255).collect();
    for bytes in [b"".as_slice(), "Bonjour 世界 🦀\0".as_bytes(), &binary] {
        let ciphertext = seal(bytes, &identity);
        assert!(ciphertext.starts_with(b"age-encryption.org/v1\n"));
        assert_eq!(
            record::open(&ciphertext, &identity).unwrap().as_slice(),
            bytes
        );

        // Independent standard AGE entry points accept the exact bytes, no wrapper.
        let decryptor = age::Decryptor::new(ciphertext.as_slice()).unwrap();
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .unwrap();
        let mut plaintext = Zeroizing::new(Vec::new());
        reader.read_to_end(&mut plaintext).unwrap();
        assert_eq!(plaintext.as_slice(), bytes);
        let standard = standard_seal(bytes, &[identity.to_public()]);
        assert_eq!(
            record::open(&standard, &identity).unwrap().as_slice(),
            bytes
        );
    }
}

#[test]
fn plaintext_boundary_and_one_over_are_enforced() {
    let identity = x25519::Identity::generate();
    let bytes = Zeroizing::new(vec![0xa5; config::MAX_RECORD_PLAINTEXT_BYTES]);
    let ciphertext = seal(&bytes, &identity);
    assert_eq!(*record::open(&ciphertext, &identity).unwrap(), *bytes);
    assert!(matches!(
        record::seal(
            Zeroizing::new(vec![0xa5; config::MAX_RECORD_PLAINTEXT_BYTES + 1]),
            &identity.to_public()
        ),
        Err(Error::Limit { .. })
    ));
    let oversized = standard_seal(
        &vec![0xa5; config::MAX_RECORD_PLAINTEXT_BYTES + 1],
        &[identity.to_public()],
    );
    assert!(record::open(&oversized, &identity).is_err());
}

#[test]
fn wrong_identity_and_every_truncation_fail_without_returning_plaintext() {
    let identity = x25519::Identity::generate();
    let ciphertext = seal(b"synthetic private record", &identity);
    assert!(record::open(&ciphertext, &x25519::Identity::generate()).is_err());
    for end in 0..ciphertext.len() {
        assert!(record::open(&ciphertext[..end], &identity).is_err());
    }
}

#[test]
fn every_byte_tamper_and_suffix_fail_closed() {
    let identity = x25519::Identity::generate();
    let ciphertext = seal(b"synthetic private record", &identity);
    for index in 0..ciphertext.len() {
        let mut corrupt = ciphertext.clone();
        corrupt[index] ^= 1;
        assert!(record::open(&corrupt, &identity).is_err());
    }
    for suffix in [b"\0".as_slice(), b"trailing", &ciphertext] {
        let mut corrupt = ciphertext.clone();
        corrupt.extend_from_slice(suffix);
        assert!(record::open(&corrupt, &identity).is_err());
    }
    // Exact full-frame termination is distinct in AGE and must also reject suffixes.
    let mut full = seal(&vec![7; config::AGE_STREAM_CHUNK_BYTES], &identity);
    full.push(0);
    assert!(record::open(&full, &identity).is_err());
}

#[test]
fn maximum_record_tamper_and_truncation_include_the_final_authentication_tag() {
    let identity = x25519::Identity::generate();
    let bytes = Zeroizing::new(vec![0xa5; config::MAX_RECORD_PLAINTEXT_BYTES]);
    let ciphertext = seal(&bytes, &identity);
    assert_eq!(*record::open(&ciphertext, &identity).unwrap(), *bytes);
    // Keep the genuine header unchanged so failures exercise payload authentication,
    // including failures after earlier chunks have already decrypted internally.
    let payload_start = ciphertext.len() - config::MAX_RECORD_PAYLOAD_BYTES;
    let tag_start = ciphertext.len() - 16;
    for index in [
        payload_start,
        payload_start + 15,
        payload_start + 16,
        payload_start + 16 + config::MAX_RECORD_PLAINTEXT_BYTES / 2,
        tag_start - 1,
    ]
    .into_iter()
    .chain(tag_start..ciphertext.len())
    {
        let mut corrupt = ciphertext.clone();
        corrupt[index] ^= 1;
        assert!(matches!(
            record::open(&corrupt, &identity),
            Err(Error::Encryption)
        ));
    }
    for end in [
        payload_start,
        payload_start + 16,
        payload_start + 16 + config::MAX_RECORD_PLAINTEXT_BYTES / 2,
        tag_start - 1,
    ]
    .into_iter()
    .chain(tag_start..ciphertext.len())
    {
        assert!(matches!(
            record::open(&ciphertext[..end], &identity),
            Err(Error::Encryption)
        ));
    }
    let stream_start = payload_start + 16;
    let encrypted_chunk_bytes = config::AGE_STREAM_CHUNK_BYTES + 16;
    for chunk in 0..config::MAX_RECORD_CHUNKS {
        let start = stream_start + chunk * encrypted_chunk_bytes;
        let end = (start + encrypted_chunk_bytes).min(ciphertext.len());
        for index in [start, (start + end - 16) / 2, end - 16, end - 1] {
            let mut corrupt = ciphertext.clone();
            corrupt[index] ^= 1;
            assert!(record::open(&corrupt, &identity).is_err());
        }
        for cut in [start, end - 17, end - 16, end - 1, end] {
            if cut < ciphertext.len() {
                assert!(record::open(&ciphertext[..cut], &identity).is_err());
            }
        }
    }
    assert!(record::open(&ciphertext, &x25519::Identity::generate()).is_err());
}

#[test]
fn every_chunk_boundary_and_neighbors_roundtrip_up_to_the_record_cap() {
    let identity = x25519::Identity::generate();
    let mut sizes = vec![0, 1, config::MAX_RECORD_PLAINTEXT_BYTES];
    for boundary in (config::AGE_STREAM_CHUNK_BYTES..=config::MAX_RECORD_PLAINTEXT_BYTES)
        .step_by(config::AGE_STREAM_CHUNK_BYTES)
    {
        sizes.extend([boundary - 1, boundary, boundary + 1]);
    }
    for size in sizes {
        let bytes = Zeroizing::new(vec![0xa5; size]);
        let ciphertext = seal(&bytes, &identity);
        assert_eq!(*record::open(&ciphertext, &identity).unwrap(), *bytes);
        let chunks = size.div_ceil(config::AGE_STREAM_CHUNK_BYTES).max(1);
        assert!(ciphertext.len() <= config::MAX_RECORD_HEADER_BYTES + 16 + size + chunks * 16);
        // Check the standard library in both directions across exact-final boundaries.
        let standard = standard_seal(&bytes, &[identity.to_public()]);
        assert_eq!(*record::open(&standard, &identity).unwrap(), *bytes);
        let decryptor = age::Decryptor::new(ciphertext.as_slice()).unwrap();
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .unwrap();
        let mut opened = Zeroizing::new(vec![0; size]);
        reader.read_exact(&mut opened).unwrap();
        let mut eof = [0];
        assert_eq!(reader.read(&mut eof).unwrap(), 0);
        assert_eq!(*opened, *bytes);
    }
}

#[test]
fn suffixes_full_frames_and_concatenated_age_never_return_a_prefix() {
    let identity = x25519::Identity::generate();
    let full_frame = vec![0; config::AGE_STREAM_CHUNK_BYTES + 16];
    let other_record = seal(b"another valid record", &identity);
    for size in [
        0,
        1,
        config::AGE_STREAM_CHUNK_BYTES - 1,
        config::AGE_STREAM_CHUNK_BYTES,
        config::AGE_STREAM_CHUNK_BYTES + 1,
        config::AGE_STREAM_CHUNK_BYTES * 2,
        config::MAX_RECORD_PLAINTEXT_BYTES,
    ] {
        let ciphertext = seal(&vec![0xa5; size], &identity);
        for suffix in [b"\0".as_slice(), b"trailing", &full_frame, &other_record] {
            let mut appended = ciphertext.clone();
            appended.extend_from_slice(suffix);
            if size <= config::AGE_STREAM_CHUNK_BYTES * 2 {
                // These must fail by authentication/EOF, not merely the input cap.
                assert!(appended.len() < config::MAX_RECORD_CIPHERTEXT_BYTES);
                assert!(matches!(
                    record::open(&appended, &identity),
                    Err(Error::Encryption)
                ));
            } else {
                assert!(record::open(&appended, &identity).is_err());
            }
        }
    }
}

#[test]
fn valid_ascii_armor_is_explicitly_outside_the_record_subset() {
    let identity = x25519::Identity::generate();
    let ciphertext = seal(b"synthetic armor fixture", &identity);
    let mut armored = Vec::new();
    let mut writer =
        age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
            .unwrap();
    writer.write_all(&ciphertext).unwrap();
    writer.finish().unwrap();
    assert!(armored.starts_with(b"-----BEGIN AGE ENCRYPTED FILE-----"));
    // Prove it is valid armor, rather than testing rejection of malformed input.
    let mut decoded = Vec::new();
    age::armor::ArmoredReader::new(armored.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, ciphertext);
    assert!(matches!(
        record::open(&armored, &identity),
        Err(Error::Encryption)
    ));
}

#[test]
fn synthetic_serialized_identity_debug_is_redacted_not_identity_debug() {
    use age::secrecy::ExposeSecret;

    // Identity itself has no Debug (compile-fail doctest in envelope::record).
    // Its explicit serialization returns SecretString, whose Debug is redacted.
    let synthetic_identity = x25519::Identity::generate();
    let serialized = synthetic_identity.to_string();
    let diagnostic = format!("{serialized:?}");
    assert!(!diagnostic.contains(serialized.expose_secret()));
    assert!(!diagnostic.contains("AGE-SECRET-KEY-"));
    assert!(diagnostic.contains("REDACTED"));
}

#[test]
fn untrusted_header_work_and_ciphertext_are_bounded() {
    let identity = x25519::Identity::generate();
    for size in [
        config::MAX_RECORD_CIPHERTEXT_BYTES,
        config::MAX_RECORD_CIPHERTEXT_BYTES + 1,
    ] {
        assert!(record::open(&vec![0; size], &identity).is_err());
    }
    for size in [
        config::MAX_RECORD_HEADER_BYTES,
        config::MAX_RECORD_HEADER_BYTES + 1,
    ] {
        let mut bomb = b"age-encryption.org/v1\n-> X25519 ".to_vec();
        bomb.resize(size, b'A');
        bomb.extend_from_slice(b"\n--- AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n");
        bomb.extend_from_slice(&[0; 32]);
        assert!(record::open(&bomb, &identity).is_err());
    }
    // No scrypt identity is constructed, even for an absurd attacker work factor.
    for stanza in [
        "-> scrypt AAAAAAAAAAAAAAAAAAAAAA 255\nAA\n",
        "-> plugin-attacker arbitrary\nAA\n",
        "-> X25519 x\nAA\n-> X25519 y\nAA\n",
        "-> X25519 x\nAA\n-> a-grease\n\n-> b-grease\n\n",
    ] {
        let mut bomb = format!(
            "age-encryption.org/v1\n{stanza}--- AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n"
        )
        .into_bytes();
        bomb.extend_from_slice(&[0; 32]);
        assert!(record::open(&bomb, &identity).is_err());
    }
    let two_recipients = standard_seal(
        b"fixture",
        &[
            identity.to_public(),
            x25519::Identity::generate().to_public(),
        ],
    );
    assert!(record::open(&two_recipients, &identity).is_err());
}

#[test]
fn returned_plaintext_has_an_observable_zeroizing_owner() {
    let identity = x25519::Identity::generate();
    let ciphertext = seal(b"synthetic private record", &identity);
    let mut plaintext: Zeroizing<Vec<u8>> = record::open(&ciphertext, &identity).unwrap();
    plaintext.as_mut_slice().zeroize();
    assert!(plaintext.iter().all(|byte| *byte == 0));
    // Reading freed memory to test Drop would be undefined behavior. The public return
    // type and error-path local use the same zeroize RAII owner, without unsafe probes.
}

#[test]
#[ignore = "manual current-thread latency observation; no absolute timing threshold"]
fn maximum_record_current_thread_latency_20_iterations() {
    use std::time::{Duration, Instant};

    const ITERATIONS: usize = 20;
    let identity = x25519::Identity::generate();
    let recipient = identity.to_public();
    let mut seal_times = [Duration::ZERO; ITERATIONS];
    let mut open_times = [Duration::ZERO; ITERATIONS];
    for index in 0..ITERATIONS {
        // Fixture preparation is outside the timer; seal includes its consumed
        // plaintext's drop, while open excludes the caller's returned-owner drop.
        let plaintext = Zeroizing::new(vec![0xa5; config::MAX_RECORD_PLAINTEXT_BYTES]);
        let start = Instant::now();
        let ciphertext = record::seal(plaintext, &recipient).unwrap();
        seal_times[index] = start.elapsed();
        let start = Instant::now();
        let opened = record::open(&ciphertext, &identity).unwrap();
        open_times[index] = start.elapsed();
        assert_eq!(opened.len(), config::MAX_RECORD_PLAINTEXT_BYTES);
        assert!(opened.iter().all(|byte| *byte == 0xa5));
    }
    // Numeric timing summaries only, never keys, ciphertext or plaintext.
    for (operation, mut times) in [("seal", seal_times), ("open", open_times)] {
        times.sort_unstable();
        let mean = times.iter().sum::<Duration>() / ITERATIONS as u32;
        let median = (times[ITERATIONS / 2 - 1] + times[ITERATIONS / 2]) / 2;
        println!(
            "record_latency operation={operation} iterations={ITERATIONS} bytes={} \
             debug_assertions={} min_us={:.3} median_us={:.3} mean_us={:.3} max_us={:.3}",
            config::MAX_RECORD_PLAINTEXT_BYTES,
            cfg!(debug_assertions),
            times[0].as_secs_f64() * 1_000_000.0,
            median.as_secs_f64() * 1_000_000.0,
            mean.as_secs_f64() * 1_000_000.0,
            times[ITERATIONS - 1].as_secs_f64() * 1_000_000.0,
        );
    }
}
