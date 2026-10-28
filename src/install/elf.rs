//! Bounded ELF64 dynamic/version metadata, without loading or executing bytes.
use super::{Error, Result, refuse};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Requirements {
    pub needed: Vec<String>,
    pub required: BTreeMap<String, BTreeSet<String>>,
    pub provided: BTreeSet<String>,
    pub glibc: Option<String>,
}

struct Segment {
    address: u64,
    offset: u64,
    size: u64,
}
struct Elf<'a> {
    bytes: &'a [u8],
    loads: Vec<Segment>,
    tags: BTreeMap<u64, u64>,
    strings: &'a [u8],
}

fn u16at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or(Error::Refused("truncated ELF"))?
            .try_into()
            .expect("two bytes"),
    ))
}
fn u32at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or(Error::Refused("truncated ELF"))?
            .try_into()
            .expect("four bytes"),
    ))
}
fn u64at(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or(Error::Refused("truncated ELF"))?
            .try_into()
            .expect("eight bytes"),
    ))
}

impl<'a> Elf<'a> {
    fn mapped(&self, address: u64, size: usize) -> Result<&'a [u8]> {
        let mut found = None;
        for segment in &self.loads {
            let Some(relative) = address.checked_sub(segment.address) else {
                continue;
            };
            if relative
                .checked_add(size as u64)
                .is_some_and(|end| end <= segment.size)
            {
                let start = segment
                    .offset
                    .checked_add(relative)
                    .ok_or(Error::Refused("ELF offset overflow"))?;
                let end = start
                    .checked_add(size as u64)
                    .ok_or(Error::Refused("ELF offset overflow"))?;
                if end > self.bytes.len() as u64 || found.is_some() {
                    return refuse("ambiguous ELF mapping");
                }
                found = Some(&self.bytes[start as usize..end as usize]);
            }
        }
        found.ok_or(Error::Refused("unmapped ELF metadata"))
    }
    fn string(&self, offset: u64) -> Result<String> {
        if offset >= self.strings.len() as u64 {
            return refuse("invalid ELF string offset");
        }
        let tail = &self.strings[offset as usize..];
        let length = tail
            .iter()
            .take(4097)
            .position(|b| *b == 0)
            .ok_or(Error::Refused("unbounded ELF string"))?;
        let text = std::str::from_utf8(&tail[..length])
            .map_err(|_| Error::Refused("invalid ELF string"))?;
        if text.is_empty()
            || !text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        {
            return refuse("unsafe dynamic loader name");
        }
        Ok(text.to_owned())
    }
    fn version_needs(&self) -> Result<BTreeMap<String, BTreeSet<String>>> {
        let mut required = BTreeMap::new();
        let mut address = *self.tags.get(&0x6fff_fffe).unwrap_or(&0);
        let count = *self.tags.get(&0x6fff_ffff).unwrap_or(&0);
        if count > 4096 || (count == 0) != (address == 0) {
            return refuse("invalid ELF version requirements");
        }
        let mut seen = BTreeSet::new();
        let mut total = 0usize;
        for index in 0..count {
            if !seen.insert(address) {
                return refuse("ELF version requirement cycle");
            }
            let record = self.mapped(address, 16)?;
            if u16at(record, 0)? != 1 {
                return refuse("unsupported ELF version record");
            }
            let library = self.string(u32at(record, 4)? as u64)?;
            let versions = required.entry(library).or_insert_with(BTreeSet::new);
            let aux_count = u16at(record, 2)?;
            let mut aux = address
                .checked_add(u32at(record, 8)? as u64)
                .ok_or(Error::Refused("ELF version offset overflow"))?;
            for aux_index in 0..aux_count {
                total += 1;
                if total > 4096 || aux <= address {
                    return refuse("oversized ELF version requirements");
                }
                let entry = self.mapped(aux, 16)?;
                versions.insert(self.string(u32at(entry, 8)? as u64)?);
                let next = u32at(entry, 12)? as u64;
                if (aux_index + 1 == aux_count) != (next == 0) {
                    return refuse("inconsistent ELF version chain");
                }
                aux = aux
                    .checked_add(next)
                    .ok_or(Error::Refused("ELF version offset overflow"))?;
            }
            let next = u32at(record, 12)? as u64;
            if (index + 1 == count) != (next == 0) {
                return refuse("inconsistent ELF version chain");
            }
            address = address
                .checked_add(next)
                .ok_or(Error::Refused("ELF version offset overflow"))?;
        }
        Ok(required)
    }
    fn version_definitions(&self) -> Result<BTreeSet<String>> {
        let mut provided = BTreeSet::new();
        let mut address = *self.tags.get(&0x6fff_fffc).unwrap_or(&0);
        let count = *self.tags.get(&0x6fff_fffd).unwrap_or(&0);
        if count > 4096 || (count == 0) != (address == 0) {
            return refuse("invalid ELF version definitions");
        }
        for index in 0..count {
            let record = self.mapped(address, 20)?;
            if u16at(record, 0)? != 1 || u16at(record, 6)? == 0 {
                return refuse("invalid ELF version definition");
            }
            let aux = address
                .checked_add(u32at(record, 12)? as u64)
                .ok_or(Error::Refused("ELF version offset overflow"))?;
            let entry = self.mapped(aux, 8)?;
            provided.insert(self.string(u32at(entry, 0)? as u64)?);
            let next = u32at(record, 16)? as u64;
            if (index + 1 == count) != (next == 0) || next != 0 && next < 20 {
                return refuse("invalid ELF definition chain");
            }
            address = address
                .checked_add(next)
                .ok_or(Error::Refused("ELF version offset overflow"))?;
        }
        Ok(provided)
    }
}

pub(super) fn inspect(bytes: &[u8], target: &str, executable: bool) -> Result<Requirements> {
    let machine = if target == "x86_64-unknown-linux-gnu" {
        62
    } else if target == "aarch64-unknown-linux-gnu" {
        183
    } else {
        return refuse("unsupported ELF target");
    };
    if bytes.len() < 64
        || &bytes[..4] != b"\x7fELF"
        || bytes[4..7] != [2, 1, 1]
        || ![0, 3].contains(&bytes[7])
        || ![2, 3].contains(&u16at(bytes, 16)?)
        || u16at(bytes, 18)? != machine
        || u16at(bytes, 52)? != 64
    {
        return refuse("ELF header differs from Linux host ABI");
    }
    let start = u64at(bytes, 32)?;
    let count = u16at(bytes, 56)? as usize;
    if u16at(bytes, 54)? != 56
        || count == 0
        || count > 1024
        || start
            .checked_add(count as u64 * 56)
            .is_none_or(|end| end > bytes.len() as u64)
    {
        return refuse("invalid ELF program headers");
    }
    let mut elf = Elf {
        bytes,
        loads: Vec::new(),
        tags: BTreeMap::new(),
        strings: &[],
    };
    let mut dynamic = None;
    let mut interpreter = false;
    let entry = u64at(bytes, 24)?;
    let mut executable_loads = 0usize;
    let mut entry_mappings = 0usize;
    for index in 0..count {
        let record = &bytes[start as usize + index * 56..start as usize + (index + 1) * 56];
        let kind = u32at(record, 0)?;
        let offset = u64at(record, 8)?;
        let size = u64at(record, 32)?;
        if offset
            .checked_add(size)
            .is_none_or(|end| end > bytes.len() as u64)
        {
            return refuse("ELF segment exceeds file");
        }
        match kind {
            1 => {
                let address = u64at(record, 16)?;
                let memory_size = u64at(record, 40)?;
                if size > memory_size || address.checked_add(memory_size).is_none() {
                    return refuse("invalid ELF load memory/file bounds");
                }
                if u32at(record, 4)? & 1 != 0 && size > 0 {
                    executable_loads += 1;
                    // Entry must be file-backed executable bytes, not BSS or a
                    // zero-size segment. Checked filesz <= memsz also bounds it
                    // inside the mapped memory extent, for static ET_EXEC/PIE.
                    if entry >= address && entry < address + size {
                        entry_mappings += 1;
                    }
                }
                elf.loads.push(Segment {
                    address,
                    offset,
                    size,
                });
            }
            2 => {
                if dynamic.replace((offset, size)).is_some() || size > 1024 * 1024 || size % 16 != 0
                {
                    return refuse("invalid ELF dynamic segment");
                }
            }
            3 => {
                let wanted = if machine == 62 {
                    b"/lib64/ld-linux-x86-64.so.2\0".as_slice()
                } else {
                    b"/lib/ld-linux-aarch64.so.1\0".as_slice()
                };
                if interpreter
                    || &bytes[offset as usize..(offset + size) as usize] != wanted
                    || !std::path::Path::new(
                        std::str::from_utf8(&wanted[..wanted.len() - 1]).expect("literal loader"),
                    )
                    .is_file()
                {
                    return refuse("unsupported or missing ELF interpreter");
                }
                interpreter = true;
            }
            _ => (),
        }
    }
    if executable && (entry == 0 || executable_loads == 0 || entry_mappings != 1) {
        return refuse("ELF entrypoint is not in a unique file-backed executable PT_LOAD");
    }
    let mut needed_offsets = Vec::new();
    if let Some((offset, size)) = dynamic {
        let mut terminated = false;
        for record in bytes[offset as usize..(offset + size) as usize]
            .as_chunks::<16>()
            .0
        {
            let tag = u64at(record, 0)?;
            let value = u64at(record, 8)?;
            if tag == 0 {
                terminated = true;
                break;
            }
            if tag == 1 {
                if needed_offsets.len() >= 64 {
                    return refuse("too many ELF dependencies");
                }
                needed_offsets.push(value);
            } else if [
                5,
                10,
                15,
                29,
                0x6fff_fffc,
                0x6fff_fffd,
                0x6fff_fffe,
                0x6fff_ffff,
            ]
            .contains(&tag)
            {
                if elf.tags.insert(tag, value).is_some() {
                    return refuse("duplicate ELF dynamic metadata");
                }
            } else if [0x6fff_fefb, 0x6fff_fefc, 0x7fff_ffff, 0x7fff_fffd].contains(&tag) {
                return refuse("unsupported loader hooks");
            }
        }
        if !terminated {
            return refuse("unterminated ELF dynamic table");
        }
        let string_size = *elf
            .tags
            .get(&10)
            .ok_or(Error::Refused("missing ELF string size"))?;
        if string_size == 0 || string_size > 4 * 1024 * 1024 {
            return refuse("oversized ELF strings");
        }
        elf.strings = elf.mapped(
            *elf.tags
                .get(&5)
                .ok_or(Error::Refused("missing ELF strings"))?,
            string_size as usize,
        )?;
        for tag in [15, 29] {
            if elf.tags.get(&tag).is_some_and(|offset| *offset != 0) {
                return refuse("custom library search path unsupported");
            }
        }
    } else if interpreter {
        return refuse("interpreter without dynamic metadata");
    }
    let needed = needed_offsets
        .into_iter()
        .map(|offset| elf.string(offset))
        .collect::<Result<Vec<_>>>()?;
    let required = elf.version_needs()?;
    if required.keys().any(|name| !needed.contains(name)) {
        return refuse("version requirement names an undeclared library");
    }
    let mut glibc: Option<(Vec<u32>, String)> = None;
    for name in required.values().flatten() {
        if let Some(value) = name.strip_prefix("GLIBC_") {
            if !executable {
                continue;
            }
            if value == "ABI_DT_RELR" {
                return refuse("GLIBC ABI extension requires explicit qualification");
            }
            let numbers = value
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| Error::Refused("unsupported GLIBC version requirement"))?;
            if !(2..=3).contains(&numbers.len()) {
                return refuse("invalid GLIBC requirement");
            }
            let mut compare = numbers;
            compare.resize(3, 0);
            if glibc.as_ref().is_none_or(|old| old.0 < compare) {
                glibc = Some((compare, value.to_owned()));
            }
        }
    }
    Ok(Requirements {
        needed,
        required,
        provided: elf.version_definitions()?,
        glibc: glibc.map(|(_, value)| value),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_host_executable_remains_accepted() {
        inspect(
            &std::fs::read("/bin/true").unwrap(),
            super::super::host_target().unwrap(),
            true,
        )
        .unwrap();
    }

    #[test]
    fn rejects_non_executable_and_out_of_load_entrypoints() {
        let original = std::fs::read("/bin/true").unwrap();
        let target = super::super::host_target().unwrap();
        let start = u64at(&original, 32).unwrap() as usize;
        let count = u16at(&original, 56).unwrap() as usize;
        for case in [
            "no-load",
            "no-exec",
            "outside",
            "zero-pie",
            "bss",
            "file-over-memory",
            "address-overflow",
        ] {
            let mut bytes = original.clone();
            let mut changed = false;
            for index in 0..count {
                let offset = start + index * 56;
                if u32at(&bytes, offset).unwrap() != 1 {
                    continue;
                }
                match case {
                    "no-load" => bytes[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes()),
                    "no-exec" => {
                        let flags = u32at(&bytes, offset + 4).unwrap() & !1;
                        bytes[offset + 4..offset + 8].copy_from_slice(&flags.to_le_bytes());
                    }
                    "bss" | "file-over-memory" | "address-overflow"
                        if !changed && u32at(&bytes, offset + 4).unwrap() & 1 != 0 =>
                    {
                        changed = true;
                        let size = u64at(&bytes, offset + 32).unwrap();
                        let address = u64at(&bytes, offset + 16).unwrap();
                        if case == "bss" {
                            bytes[offset + 40..offset + 48]
                                .copy_from_slice(&(size + 4096).to_le_bytes());
                            bytes[24..32].copy_from_slice(&(address + size).to_le_bytes());
                        } else if case == "file-over-memory" {
                            bytes[offset + 40..offset + 48]
                                .copy_from_slice(&(size - 1).to_le_bytes());
                        } else {
                            bytes[offset + 16..offset + 24]
                                .copy_from_slice(&u64::MAX.to_le_bytes());
                        }
                    }
                    _ => (),
                }
            }
            if case == "outside" {
                bytes[24..32].copy_from_slice(&(u64::MAX - 1).to_le_bytes());
            }
            if case == "zero-pie" {
                bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
                bytes[24..32].fill(0);
            }
            assert!(
                inspect(&bytes, target, true).is_err(),
                "accepted malformed fixture: {case}"
            );
        }
    }

    #[test]
    fn rejects_reported_zero_sized_nonload_et_exec() {
        let original = std::fs::read("/bin/true").unwrap();
        // Negative fixture only: retain a real ELF header and replace its PHDR
        // table with one empty PT_NULL. This must never count as valid static ELF.
        let mut bytes = original[..120].to_vec();
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..].fill(0);
        assert!(inspect(&bytes, super::super::host_target().unwrap(), true).is_err());
    }
}
