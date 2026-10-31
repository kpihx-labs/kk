// Copyright (c) 2026 Mike Grier; local K&K extension, MIT.
//! Owned typed values from one self-relative descriptor; no pointer dereferences.
use std::io;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid self-relative security descriptor",
    )
}
fn part(bytes: &[u8], offset: usize, length: usize) -> io::Result<&[u8]> {
    bytes
        .get(offset..offset.checked_add(length).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}
fn word(bytes: &[u8], at: usize) -> io::Result<u16> {
    Ok(u16::from_le_bytes(
        part(bytes, at, 2)?.try_into().map_err(|_| invalid())?,
    ))
}
fn dword(bytes: &[u8], at: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(
        part(bytes, at, 4)?.try_into().map_err(|_| invalid())?,
    ))
}

/// Validated revision-one SID, owned independently of the native snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sid(Vec<u8>);
impl Sid {
    /// Decode an exact SID, rejecting truncated or trailing data.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 8
            || bytes[0] != 1
            || bytes[1] > 15
            || bytes.len() != 8 + 4 * bytes[1] as usize
        {
            return Err(invalid());
        }
        Ok(Self(bytes.to_vec()))
    }
    /// Native SID bytes, not a borrowed native pointer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Numeric SDDL spelling; never used as a descriptor-equality shortcut.
    pub fn to_sddl(&self) -> String {
        let authority = self.0[2..8]
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
        let mut text = format!("S-1-{authority}");
        for number in self.0[8..].chunks_exact(4) {
            text.push_str(&format!(
                "-{}",
                u32::from_le_bytes(number.try_into().expect("SID chunk"))
            ));
        }
        text
    }
}
fn sid_at(bytes: &[u8], offset: usize) -> io::Result<Sid> {
    let header = part(bytes, offset, 8)?;
    Sid::from_bytes(part(bytes, offset, 8 + 4 * header[1] as usize)?)
}

/// ACEs not interpreted here remain explicitly unsupported, never implicit grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ace {
    /// Ordinary ACCESS_ALLOWED_ACE.
    Allow { flags: u8, mask: u32, sid: Sid },
    /// Ordinary ACCESS_DENIED_ACE.
    Deny { flags: u8, mask: u32, sid: Sid },
    /// Object, callback, mandatory-label or future ACE; policy must decide/refuse.
    Unsupported { kind: u8, flags: u8 },
}
/// An absent or NULL ACL grants differently from an empty present ACL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dacl {
    Absent,
    Null,
    Present { revision: u8, aces: Vec<Ace> },
}
/// Owner, control and DACL from exactly one immutable owned native snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecuritySnapshot {
    pub control: u16,
    pub owner: Option<Sid>,
    pub dacl: Dacl,
}
impl SecuritySnapshot {
    /// Bounds-check the self-relative representation, without SDDL comparison.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 20 || bytes[0] != 1 || bytes[1] != 0 {
            return Err(invalid());
        }
        let control = word(bytes, 2)?;
        if control & 0x8000 == 0 {
            return Err(invalid());
        }
        let owner_offset = dword(bytes, 4)? as usize;
        let owner = if owner_offset == 0 {
            None
        } else {
            if owner_offset < 20 {
                return Err(invalid());
            }
            Some(sid_at(bytes, owner_offset)?)
        };
        let offset = dword(bytes, 16)? as usize;
        let dacl = if control & 4 == 0 {
            if offset != 0 {
                return Err(invalid());
            }
            Dacl::Absent
        } else if offset == 0 {
            Dacl::Null
        } else {
            if offset < 20 {
                return Err(invalid());
            }
            let header = part(bytes, offset, 8)?;
            if !matches!(header[0], 2 | 4) {
                return Err(invalid());
            }
            let size = word(header, 2)? as usize;
            let count = word(header, 4)? as usize;
            if size < 8 || count > (size - 8) / 4 {
                return Err(invalid());
            }
            let acl = part(bytes, offset, size)?;
            let mut cursor = 8;
            let mut aces = Vec::new();
            for _ in 0..count {
                let ace_header = part(acl, cursor, 4)?;
                let length = word(ace_header, 2)? as usize;
                if length < 4 || length % 4 != 0 {
                    return Err(invalid());
                }
                let data = part(acl, cursor, length)?;
                let kind = data[0];
                let flags = data[1];
                let ace = if matches!(kind, 0 | 1) {
                    let mask = dword(data, 4)?;
                    let sid = Sid::from_bytes(data.get(8..).ok_or_else(invalid)?)?;
                    if kind == 0 {
                        Ace::Allow { flags, mask, sid }
                    } else {
                        Ace::Deny { flags, mask, sid }
                    }
                } else {
                    Ace::Unsupported { kind, flags }
                };
                aces.push(ace);
                cursor += length;
            }
            Dacl::Present {
                revision: header[0],
                aces,
            }
        };
        Ok(Self {
            control,
            owner,
            dacl,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn null_empty_and_owner_allow_are_distinct_and_truncation_is_refused() {
        let mut bytes = vec![
            1, 0, 4, 0x90, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let sid = [1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0];
        bytes.extend(sid);
        assert_eq!(
            SecuritySnapshot::from_bytes(&bytes).unwrap().dacl,
            Dacl::Null
        );
        bytes[16] = 32;
        bytes.extend([2, 0, 8, 0, 0, 0, 0, 0]);
        assert!(
            matches!(SecuritySnapshot::from_bytes(&bytes).unwrap().dacl, Dacl::Present { aces, .. } if aces.is_empty())
        );
        bytes[34] = 28;
        bytes[36] = 1;
        bytes.extend([0, 0, 20, 0, 0xff, 1, 0x1f, 0]);
        bytes.extend(sid);
        let snapshot = SecuritySnapshot::from_bytes(&bytes).unwrap();
        assert!(
            matches!(&snapshot.dacl, Dacl::Present { aces, .. } if matches!(&aces[..], [Ace::Allow { flags: 0, mask: 0x1f01ff, sid }] if Some(sid) == snapshot.owner.as_ref()))
        );
        assert!(SecuritySnapshot::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    }
}
