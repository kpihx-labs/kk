//! Decode bounded in-memory bytes, never extract through archive library APIs.
use super::{Result, digest, refuse};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

pub(super) const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_MANIFEST: usize = 4 * 1024 * 1024;
const MAX_MEMBERS: usize = 4096;

#[derive(Debug, Deserialize)]
pub(super) struct Manifest {
    pub format: u32,
    pub version: String,
    pub target: String,
    pub commit: String,
    pub binary_glibc_required: Option<String>,
    pub binary_macos_required: Option<String>,
    #[serde(deserialize_with = "unique_files")]
    pub files: BTreeMap<String, String>,
}

fn unique_files<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("unique file digests")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut files = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if files.len() >= MAX_MEMBERS || files.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate or excessive manifest files",
                    ));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(Visitor)
}

pub(super) struct Verified {
    pub manifest: Manifest,
    // Includes the original build manifest, excluded only from its own digest map.
    pub files: BTreeMap<String, Vec<u8>>,
}

pub(super) fn path(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 4096
        || name.split('/').count() > 32
        || name.contains(['\\', ':'])
        || name.chars().any(|c| c.is_control())
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with(['.', ' ']))
    {
        return refuse("unsafe member path");
    }
    Ok(())
}

fn bounded(input: impl Read, limit: usize, cancel: &super::Cancel) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut input = input.take(limit as u64 + 1);
    let mut chunk = [0u8; 65536];
    loop {
        cancel.check()?;
        let count = input.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() > limit {
        return refuse("archive resource budget exceeded");
    }
    Ok(bytes)
}

// Preflight BEFORE ZipArchive allocates one object per central-directory entry.
fn zip_directory(bytes: &[u8]) -> Result<()> {
    let start = bytes.len().saturating_sub(65557);
    let Some(end) = bytes[start..]
        .windows(4)
        .rposition(|b| b == b"PK\x05\x06")
        .map(|p| p + start)
    else {
        return refuse("missing ZIP directory");
    };
    if end + 22 > bytes.len() {
        return refuse("truncated ZIP directory");
    }
    let u16at = |p| u16::from_le_bytes([bytes[p], bytes[p + 1]]) as usize;
    let u32at = |p| {
        u32::from_le_bytes(bytes[p..p + 4].try_into().expect("checked directory range")) as usize
    };
    let count = u16at(end + 10);
    let length = u32at(end + 12);
    let offset = u32at(end + 16);
    if u16at(end + 4) != 0
        || u16at(end + 6) != 0
        || u16at(end + 8) != count
        || count > MAX_MEMBERS
        || length > MAX_MANIFEST
        || offset.checked_add(length) != Some(end)
        || end + 22 + u16at(end + 20) != bytes.len()
    {
        return refuse("ambiguous or oversized ZIP directory");
    }
    let mut position = offset;
    for _ in 0..count {
        if position + 46 > end || &bytes[position..position + 4] != b"PK\x01\x02" {
            return refuse("invalid ZIP directory");
        }
        // No ZIP64, encryption, multipart, non-UTF8 names or unsupported methods.
        let flags = u16at(position + 8);
        let method = u16at(position + 10);
        if u16at(position + 6) >= 45
            || flags & !0x080e != 0
            || flags & 1 != 0
            || ![0, 8].contains(&method)
            || u16at(position + 34) != 0
            || u32at(position + 38) & 0x10 != 0
            || [
                u32at(position + 20),
                u32at(position + 24),
                u32at(position + 42),
            ]
            .contains(&0xffff_ffff)
        {
            return refuse("unsupported ZIP feature");
        }
        position += 46 + u16at(position + 28) + u16at(position + 30) + u16at(position + 32);
        if position > end {
            return refuse("oversized ZIP directory entry");
        }
    }
    if position != end {
        return refuse("inconsistent ZIP member count");
    }
    Ok(())
}

pub(super) fn verify(
    bytes: &[u8],
    version: &str,
    commit: &str,
    target: &str,
    cancel: &super::Cancel,
) -> Result<Verified> {
    cancel.check()?;
    if bytes.len() > MAX_BYTES {
        return refuse("compressed archive is oversized");
    }
    let mut members = BTreeMap::new();
    let mut spellings = BTreeMap::new();
    let mut parents = BTreeSet::new();
    let mut total = 0usize;
    let mut accept = |name: String, size: u64, data: Vec<u8>| -> Result<()> {
        cancel.check()?;
        path(&name)?;
        if size > MAX_FILE as u64 || data.len() as u64 != size || members.len() >= MAX_MEMBERS {
            return refuse("oversized member or inconsistent length");
        }
        total = total
            .checked_add(data.len())
            .ok_or(super::Error::Refused("expanded size overflow"))?;
        if total > MAX_BYTES || members.contains_key(&name) {
            return refuse("expanded size or member collision");
        }
        // Restrict Unicode case ambiguities further: release names must be ASCII.
        if !name.is_ascii() {
            return refuse("non-ASCII archive member name");
        }
        if parents.contains(&name) {
            return refuse("file/directory conflict");
        }
        let components: Vec<_> = name.split('/').collect();
        for count in 1..=components.len() {
            let component = components[..count].join("/");
            let folded = component.to_ascii_lowercase();
            if spellings.get(&folded).is_some_and(|old| *old != component) {
                return refuse("case-colliding path components");
            }
            spellings.insert(folded, component.clone());
            if count < components.len() {
                if members.contains_key(&component) {
                    return refuse("file/directory conflict");
                }
                parents.insert(component);
            }
        }
        if name.ends_with("/build-manifest.json") && data.len() > MAX_MANIFEST {
            return refuse("manifest is oversized");
        }
        members.insert(name, data);
        Ok(())
    };
    if bytes.starts_with(b"PK") {
        zip_directory(bytes)?;
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| super::Error::Refused("invalid ZIP"))?;
        for index in 0..zip.len() {
            cancel.check()?;
            let mut member = zip
                .by_index(index)
                .map_err(|_| super::Error::Refused("invalid ZIP member"))?;
            let mode = member.unix_mode().unwrap_or(0);
            if member.is_dir()
                || mode & 0o170000 != 0 && mode & 0o170000 != 0o100000
                || mode & 0o7000 != 0
            {
                return refuse("non-ordinary ZIP member");
            }
            let name = std::str::from_utf8(member.name_raw())
                .map_err(|_| super::Error::Refused("invalid ZIP name"))?
                .to_owned();
            if name.as_bytes().contains(&0) || member.size() > MAX_FILE as u64 {
                return refuse("unsafe ZIP member");
            }
            let size = member.size();
            let data = bounded(&mut member, MAX_FILE.min(size as usize), cancel)?;
            accept(name, size, data)?;
        }
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        let unpacked = bounded(flate2::read::MultiGzDecoder::new(bytes), MAX_BYTES, cancel)?;
        // Bound hidden PAX/GNU headers BEFORE the library interprets/allocates them.
        let mut preflight = tar::Archive::new(Cursor::new(&unpacked));
        preflight.set_ignore_zeros(true);
        for (index, entry) in preflight.entries()?.raw(true).enumerate() {
            cancel.check()?;
            let entry = entry?;
            let kind = entry.header().entry_type().as_byte();
            if index >= MAX_MEMBERS * 2
                || ![0, b'0', b'x', b'L'].contains(&kind)
                || entry.header().size()?
                    > if b"xL".contains(&kind) {
                        MAX_MANIFEST as u64
                    } else {
                        MAX_FILE as u64
                    }
            {
                return refuse("unsupported or oversized TAR header");
            }
        }
        let mut tar = tar::Archive::new(Cursor::new(unpacked));
        tar.set_ignore_zeros(true);
        for entry in tar.entries()? {
            cancel.check()?;
            let mut entry = entry?;
            if ![0, b'0'].contains(&entry.header().entry_type().as_byte())
                || entry.header().mode()? & 0o7000 != 0
                || entry.size() > MAX_FILE as u64
            {
                return refuse("non-ordinary TAR member");
            }
            if let Some(extensions) = entry.pax_extensions()? {
                for extension in extensions {
                    if extension?.key_bytes().starts_with(b"GNU.sparse") {
                        return refuse("sparse TAR member");
                    }
                }
            }
            let name = std::str::from_utf8(&entry.path_bytes())
                .map_err(|_| super::Error::Refused("invalid TAR name"))?
                .to_owned();
            let size = entry.size();
            let data = bounded(&mut entry, MAX_FILE.min(size as usize), cancel)?;
            accept(name, size, data)?;
        }
    } else {
        return refuse("expected TAR gzip or ZIP archive");
    }
    let Some(first) = members.keys().next() else {
        return refuse("empty archive");
    };
    let Some((root, _)) = first.split_once('/') else {
        return refuse("missing archive root");
    };
    let prefix = format!("{root}/");
    let manifest_bytes = members
        .get(&(prefix.clone() + "build-manifest.json"))
        .ok_or(super::Error::Refused("missing build manifest"))?;
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|_| super::Error::Refused("invalid build manifest"))?;
    if manifest.format != 1
        || manifest.version != version
        || manifest.commit != commit
        || manifest.target != target
    {
        return refuse("manifest selectors differ from requested release");
    }
    if !manifest.files.contains_key("kk") || manifest.files.len() + 1 != members.len() {
        return refuse("manifest file set differs");
    }
    for (name, hash) in &manifest.files {
        cancel.check()?;
        path(name)?;
        if name == "build-manifest.json"
            || name == ".kk-receipt.json"
            || name == ".kk-archive"
            || name.starts_with(".kk-receipt.json/")
            || name.starts_with(".kk-archive/")
            || !super::sha(hash)
            || members
                .get(&(prefix.clone() + name))
                .is_none_or(|bytes| digest(bytes) != *hash)
        {
            return refuse("manifest file digest differs");
        }
    }
    let mut files = BTreeMap::new();
    for (name, data) in members {
        let relative = name
            .strip_prefix(&prefix)
            .ok_or(super::Error::Refused("multiple archive roots"))?;
        files.insert(relative.to_owned(), data);
    }
    Ok(Verified { manifest, files })
}
