//! Bounded native Mach-O policy on top of object's checked load-command reader.
//! No execution, dyld loading, shell tools or hand-written binary decoding.
use super::{Error, Result, refuse};
use object::{Endianness, macho, read::macho::MachHeader};
use std::collections::BTreeSet;

#[derive(Debug)]
pub(super) struct Requirements {
    pub minimum_os: String,
    pub needed: BTreeSet<String>,
}

fn parsed<T>(value: object::read::Result<T>) -> Result<T> {
    value.map_err(|_| Error::Refused("malformed Mach-O executable"))
}

fn system_library(name: &str) -> bool {
    (name.starts_with("/usr/lib/") || name.starts_with("/System/Library/Frameworks/"))
        && name.len() <= 4096
        && name.split('/').skip(1).all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.chars().any(|c| c.is_control() || c == '\\')
        })
}

pub(super) fn inspect(bytes: &[u8], target: &str) -> Result<Requirements> {
    if bytes.len() > 128 * 1024 * 1024 {
        return refuse("Mach-O executable exceeds byte budget");
    }
    let (cpu, subtype) = match target {
        "x86_64-apple-darwin" => (macho::CPU_TYPE_X86_64, macho::CPU_SUBTYPE_X86_64_ALL),
        "aarch64-apple-darwin" => (macho::CPU_TYPE_ARM64, macho::CPU_SUBTYPE_ARM64_ALL),
        _ => return refuse("unsupported Mach-O release target"),
    };
    // Thin, little-endian 64-bit executable only. Universal files must not choose
    // a different slice at execution time than the one authenticated/inspected.
    let header = parsed(macho::MachHeader64::<Endianness>::parse(bytes, 0))?;
    let endian = parsed(header.endian())?;
    if !header.is_little_endian()
        || header.cputype(endian) != cpu
        || header.cpusubtype(endian) & !macho::CPU_SUBTYPE_MASK != subtype
        || header.filetype(endian) != macho::MH_EXECUTE
        || header.flags(endian) & macho::MH_PIE == 0
        || header.ncmds(endian) == 0
        || header.ncmds(endian) > 4096
        || header.sizeofcmds(endian) > 1024 * 1024
    {
        return refuse("Mach-O type, CPU, PIE policy or command budget differs");
    }
    let mut commands = parsed(header.load_commands(endian, bytes, 0))?;
    let mut consumed = 0u32;
    let mut minimum = None;
    let mut loader = false;
    let mut entry = None;
    let mut executable = Vec::new();
    let mut needed = BTreeSet::new();
    while let Some(command) = parsed(commands.next())? {
        if !command.cmdsize().is_multiple_of(8) {
            return refuse("unaligned Mach-O load command");
        }
        consumed = consumed
            .checked_add(command.cmdsize())
            .ok_or(Error::Refused("Mach-O command size overflow"))?;
        // Parse all known commands, not just the ones consumed by this policy.
        parsed(command.variant())?;
        match command.cmd() {
            macho::LC_BUILD_VERSION => {
                let build = parsed(command.data::<macho::BuildVersionCommand<Endianness>>())?;
                if build.platform.get(endian) != macho::PLATFORM_MACOS || minimum.is_some() {
                    return refuse("Mach-O platform or duplicate deployment requirement");
                }
                let tools = build.ntools.get(endian);
                if tools > 128 || command.cmdsize() != 24 + tools * 8 {
                    return refuse("Mach-O build tool table differs");
                }
                minimum = Some(build.minos.get(endian));
            }
            macho::LC_VERSION_MIN_MACOSX => {
                let version = parsed(command.data::<macho::VersionMinCommand<Endianness>>())?;
                if minimum.replace(version.version.get(endian)).is_some() {
                    return refuse("duplicate Mach-O deployment requirement");
                }
            }
            macho::LC_VERSION_MIN_IPHONEOS
            | macho::LC_VERSION_MIN_TVOS
            | macho::LC_VERSION_MIN_WATCHOS => return refuse("non-macOS Mach-O deployment target"),
            macho::LC_LOAD_DYLINKER => {
                let linker = parsed(command.data::<macho::DylinkerCommand<Endianness>>())?;
                if loader || parsed(command.string(endian, linker.name))? != b"/usr/lib/dyld" {
                    return refuse("non-system or duplicate Mach-O loader");
                }
                loader = true;
            }
            macho::LC_MAIN => {
                let main = parsed(command.data::<macho::EntryPointCommand<Endianness>>())?;
                if entry.replace(main.entryoff.get(endian)).is_some() {
                    return refuse("duplicate Mach-O entrypoint");
                }
            }
            macho::LC_SEGMENT_64 => {
                let segment = parsed(command.data::<macho::SegmentCommand64<Endianness>>())?;
                let offset = segment.fileoff.get(endian);
                let size = segment.filesize.get(endian);
                let end = offset
                    .checked_add(size)
                    .ok_or(Error::Refused("Mach-O segment overflow"))?;
                if end > bytes.len() as u64
                    || size > segment.vmsize.get(endian)
                    || segment
                        .vmaddr
                        .get(endian)
                        .checked_add(segment.vmsize.get(endian))
                        .is_none()
                    || segment.nsects.get(endian) > 4096
                    || u64::from(command.cmdsize())
                        != 72 + u64::from(segment.nsects.get(endian)) * 80
                {
                    return refuse("Mach-O segment exceeds executable bounds");
                }
                let protection = segment.initprot.get(endian);
                if protection & macho::VM_PROT_EXECUTE != 0 {
                    if protection & macho::VM_PROT_WRITE != 0 {
                        return refuse("writable executable Mach-O segment");
                    }
                    executable.push(offset..end);
                }
            }
            macho::LC_RPATH
            | macho::LC_DYLD_ENVIRONMENT
            | macho::LC_UNIXTHREAD
            | macho::LC_THREAD
            | macho::LC_LOAD_DYLIB
            | macho::LC_LOAD_WEAK_DYLIB
            | macho::LC_REEXPORT_DYLIB
            | macho::LC_LAZY_LOAD_DYLIB
            | macho::LC_LOAD_UPWARD_DYLIB => {
                if let Some(library) = parsed(command.dylib())? {
                    let name =
                        std::str::from_utf8(parsed(command.string(endian, library.dylib.name))?)
                            .map_err(|_| Error::Refused("invalid Mach-O library name"))?;
                    if !system_library(name) || !needed.insert(name.to_owned()) {
                        return refuse("non-system or duplicate Mach-O dependency");
                    }
                } else {
                    return refuse("Mach-O loader environment, rpath or legacy thread entrypoint");
                }
            }
            macho::LC_SYMTAB
            | macho::LC_DYSYMTAB
            | macho::LC_UUID
            | macho::LC_CODE_SIGNATURE
            | macho::LC_DYLD_INFO
            | macho::LC_DYLD_INFO_ONLY
            | macho::LC_FUNCTION_STARTS
            | macho::LC_DATA_IN_CODE
            | macho::LC_DYLIB_CODE_SIGN_DRS
            | macho::LC_SOURCE_VERSION
            | macho::LC_LINKER_OPTIMIZATION_HINT
            | macho::LC_DYLD_EXPORTS_TRIE
            | macho::LC_DYLD_CHAINED_FIXUPS
            | macho::LC_TWOLEVEL_HINTS
            | macho::LC_LINKER_OPTION
            | macho::LC_NOTE => (),
            _ => return refuse("unqualified Mach-O load command"),
        }
    }
    let min = minimum.ok_or(Error::Refused("missing Mach-O deployment requirement"))?;
    if consumed != header.sizeofcmds(endian)
        || min >> 16 < 10
        || !loader
        || !entry.is_some_and(|entry| executable.iter().any(|range| range.contains(&entry)))
        || !needed.contains("/usr/lib/libSystem.B.dylib")
    {
        return refuse("incomplete Mach-O executable or deployment metadata");
    }
    Ok(Requirements {
        minimum_os: format!("{}.{}.{}", min >> 16, (min >> 8) & 255, min & 255),
        needed,
    })
}

pub(super) fn compatible_os(required: &str, installed: &str) -> Result<()> {
    fn version(value: &str) -> Result<[u32; 3]> {
        if value.len() > 32 {
            return refuse("invalid macOS version");
        }
        let parts = value.trim().split('.').collect::<Vec<_>>();
        if !(2..=3).contains(&parts.len()) {
            return refuse("invalid macOS version");
        }
        let mut parsed = [0; 3];
        for (index, part) in parts.into_iter().enumerate() {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return refuse("invalid macOS version");
            }
            parsed[index] = part
                .parse()
                .map_err(|_| Error::Refused("invalid macOS version"))?;
        }
        Ok(parsed)
    }
    if version(installed)? < version(required)? {
        return refuse("macOS is older than the executable deployment requirement");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(kind: u32, words: &[u32], text: Option<&str>) -> Vec<u8> {
        let mut body: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        if let Some(text) = text {
            body.extend_from_slice(text.as_bytes());
            body.push(0);
        }
        while !(body.len() + 8).is_multiple_of(8) {
            body.push(0);
        }
        [
            kind.to_le_bytes().as_slice(),
            ((body.len() + 8) as u32).to_le_bytes().as_slice(),
            &body,
        ]
        .concat()
    }

    fn image(cpu: u32, library: &str) -> Vec<u8> {
        // Synthetic parser fixture, never claimed as an executable/native replay.
        let commands = [
            command(
                macho::LC_BUILD_VERSION,
                &[macho::PLATFORM_MACOS, 13 << 16, 14 << 16, 0],
                None,
            ),
            command(macho::LC_LOAD_DYLINKER, &[12], Some("/usr/lib/dyld")),
            command(macho::LC_LOAD_DYLIB, &[24, 0, 0, 0], Some(library)),
            command(macho::LC_MAIN, &[512, 0, 0, 0], None),
            command(
                macho::LC_SEGMENT_64,
                &[0, 0, 0, 0, 0, 0, 1024, 0, 0, 0, 1024, 0, 5, 5, 0, 0],
                None,
            ),
        ]
        .concat();
        let subtype = if cpu == macho::CPU_TYPE_X86_64 {
            macho::CPU_SUBTYPE_X86_64_ALL
        } else {
            macho::CPU_SUBTYPE_ARM64_ALL
        };
        let mut bytes: Vec<u8> = [
            macho::MH_MAGIC_64,
            cpu,
            subtype,
            macho::MH_EXECUTE,
            5,
            commands.len() as u32,
            macho::MH_PIE,
            0,
        ]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
        bytes.extend(commands);
        bytes.resize(1024, 0);
        bytes
    }

    #[test]
    fn thin_baseline_images_bind_cpu_and_minimum_os() {
        for (cpu, target) in [
            (macho::CPU_TYPE_X86_64, "x86_64-apple-darwin"),
            (macho::CPU_TYPE_ARM64, "aarch64-apple-darwin"),
        ] {
            let candidate = inspect(&image(cpu, "/usr/lib/libSystem.B.dylib"), target).unwrap();
            assert_eq!(candidate.minimum_os, "13.0.0");
            assert!(candidate.needed.contains("/usr/lib/libSystem.B.dylib"));
        }
        assert!(
            inspect(
                &image(macho::CPU_TYPE_ARM64, "/usr/lib/libSystem.B.dylib"),
                "x86_64-apple-darwin"
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_and_non_system_dependencies_fail_closed() {
        for library in [
            "@rpath/libbad.dylib",
            "/usr/local/lib/bad.dylib",
            "/usr/lib/../bad.dylib",
            "relative",
        ] {
            assert!(
                inspect(
                    &image(macho::CPU_TYPE_ARM64, library),
                    "aarch64-apple-darwin"
                )
                .is_err()
            );
        }
        let bytes = image(macho::CPU_TYPE_ARM64, "/usr/lib/libSystem.B.dylib");
        for size in [0, 4, 31, 64, 100] {
            assert!(inspect(&bytes[..size], "aarch64-apple-darwin").is_err());
        }
        let mut budget = bytes.clone();
        budget[16..20].copy_from_slice(&4097u32.to_le_bytes());
        assert!(inspect(&budget, "aarch64-apple-darwin").is_err());
    }

    #[test]
    fn os_versions_compare_numerically() {
        compatible_os("13.0.0", "14.1\n").unwrap();
        assert!(compatible_os("14.0.0", "13.9.9").is_err());
        assert!(compatible_os("13.0.0", "13.bad").is_err());
    }
}
