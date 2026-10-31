//! Handle-authority Windows primitives; not a pathname-based profile bootstrap.
use crate::{Error, Result, config};
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Write},
    os::windows::{fs::MetadataExt, io::AsHandle},
};
use win_authorization_sys::{
    Ace, Dacl, SecurityAttributes, SecurityDescriptor, Sid, effective_user_sid,
    file_security_snapshot,
};
use windows_namespace_request_sys::{
    QueryFileInformationByHandle, QueryVolumeInformation, RelativeFile, create_directory_relative,
    create_file_relative, open_directory_relative, open_file_relative, validate_relative_component,
};
use zeroize::Zeroizing;

pub(crate) fn owner_attributes(owner: &Sid) -> Result<SecurityAttributes> {
    let descriptor =
        SecurityDescriptor::owner_only(owner).map_err(|error| Error::Io(error.into_source()))?;
    Ok(SecurityAttributes::new(Some(descriptor), false))
}

fn validate_owner(file: &File, owner: &Sid) -> Result<()> {
    if effective_user_sid()? != *owner {
        return Err(Error::Denied);
    }
    let snapshot = file_security_snapshot(file.as_handle())?;
    if snapshot.control & 0x9004 != 0x9004
        || snapshot.control & !0x9407 != 0
        || snapshot.owner.as_ref() != Some(owner)
    {
        return Err(Error::Destination(
            "private object owner/control policy refused",
        ));
    }
    match &snapshot.dacl {
        Dacl::Present { aces, .. } if matches!(&aces[..], [Ace::Allow { flags: 0, mask: 0x1f01ff, sid }] if sid == owner) => {
            Ok(())
        }
        _ => Err(Error::Destination(
            "private object requires one explicit owner-only full-access ACE",
        )),
    }
}

pub(crate) fn validate_security(
    snapshot: &win_authorization_sys::SecuritySnapshot,
    owner: &Sid,
) -> Result<()> {
    if snapshot.control & 0x9004 != 0x9004
        || snapshot.control & !0x9407 != 0
        || snapshot.owner.as_ref() != Some(owner)
    {
        return Err(Error::Denied);
    }
    match &snapshot.dacl {
        Dacl::Present { aces, .. } if matches!(&aces[..], [Ace::Allow { flags: 0, mask: 0x1f01ff, sid }] if sid == owner) => {
            Ok(())
        }
        _ => Err(Error::Denied),
    }
}

fn validate_file(file: &File, owner: &Sid) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err(Error::Destination(
            "private file handle is reparse or non-regular",
        ));
    }
    let information = {
        let request = QueryFileInformationByHandle::new(file.try_clone()?);
        let information = request
            .perform()
            .map_err(|error| Error::Io(error.to_io_error()))?;
        // No duplicate/request may outlive this check and reach publication.
        drop(request);
        information
    };
    if information.nNumberOfLinks != 1 {
        return Err(Error::Destination(
            "private file must have exactly one link",
        ));
    }
    validate_owner(file, owner)
}

/// A new, exclusively created owner-only file. It deliberately has no Debug or
/// raw-file accessor; a partial write cannot be retried or silently truncated.
pub struct ProtectedFile {
    native: RelativeFile,
    _directory: File,
    owner: Sid,
    written: bool,
}

impl ProtectedFile {
    /// One bounded write, including a data flush request. Errors preserve state.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if self.written || self.native.as_file().metadata()?.len() != 0 {
            return Err(Error::Destination("private file cannot be rewritten"));
        }
        if bytes.len() > config::MAX_IDENTITY_BYTES {
            return Err(Error::Identity("identity file is too large"));
        }
        validate_file(self.native.as_file(), &self.owner)?;
        self.written = true;
        let mut file = self.native.as_file();
        file.write_all(bytes)?;
        file.sync_all()?;
        validate_file(file, &self.owner)
    }
}

/// A retained owner-only directory capability. Directory metadata opens share
/// read/write/delete for native rename; its DACL remains the access authority.
/// The caller supplies an already trusted parent handle. Dropping this capability
/// does not establish that an arbitrary later DOS pathname names the same object.
pub struct ProtectedDirectory {
    file: File,
    _parent: File,
    owner: Sid,
}

impl ProtectedDirectory {
    pub(crate) fn owner(&self) -> &Sid {
        &self.owner
    }
    /// Atomically apply owner/DACL at exclusive creation, then validate the same
    /// returned handle. Failure leaves any newly created state private and intact;
    /// it never adopts, deletes or repairs an existing directory.
    pub fn create_in(parent: &File, leaf: &OsStr) -> Result<Self> {
        let volume = QueryVolumeInformation::new(parent.try_clone()?)
            .perform()
            .map_err(|error| Error::Io(error.to_io_error()))?;
        if volume.flags() & 8 == 0 {
            // FILE_PERSISTENT_ACLS.
            return Err(Error::Destination(
                "filesystem does not report persistent ACL support",
            ));
        }
        let owner = effective_user_sid()?;
        let attributes = owner_attributes(&owner)?;
        let file = File::from(create_directory_relative(
            parent.as_handle(),
            leaf,
            &attributes,
        )?);
        let candidate = Self {
            file,
            _parent: parent.try_clone()?,
            owner,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    /// Validate only this retained object; never reopen a name or mix ACL reads.
    pub fn validate(&self) -> Result<()> {
        let metadata = self.file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::Destination(
                "private directory handle is reparse or non-directory",
            ));
        }
        // SELF_RELATIVE, DACL_PROTECTED and DACL_PRESENT are mandatory. Native
        // AUTO_INHERITED is harmless only with the exact noninherited ACE below.
        // Owner/group defaulted markers grant nothing: owner SID is checked and
        // the only accepted ACE grants that SID, never the primary group.
        validate_owner(&self.file, &self.owner)
    }

    /// Create another protected child without returning to a pathname namespace.
    pub fn create_child(&self, leaf: &OsStr) -> Result<Self> {
        self.validate()?;
        Self::create_in(&self.file, leaf)
    }

    /// Open an existing protected child by handle, never creating or repairing it.
    pub fn open_child(&self, leaf: &OsStr) -> Result<Self> {
        self.validate()?;
        let file = File::from(open_directory_relative(self.file.as_handle(), leaf)?);
        let candidate = Self {
            file,
            _parent: self.file.try_clone()?,
            owner: self.owner.clone(),
        };
        candidate.validate()?;
        Ok(candidate)
    }

    /// Create a private file exclusively with its DACL applied by the kernel.
    pub fn create_private_file(&self, leaf: &OsStr) -> Result<ProtectedFile> {
        self.validate()?;
        let attributes = owner_attributes(&self.owner)?;
        let native = create_file_relative(self.file.as_handle(), leaf, &attributes)?;
        validate_file(native.as_file(), &self.owner)?;
        Ok(ProtectedFile {
            native,
            _directory: self.file.try_clone()?,
            owner: self.owner.clone(),
            written: false,
        })
    }

    /// Read only a protected regular single-link file. Missing/partial state is
    /// an error, never a request to generate a replacement key.
    pub fn read_private_file(&self, leaf: &OsStr) -> Result<Zeroizing<Vec<u8>>> {
        self.validate()?;
        let native = open_file_relative(self.file.as_handle(), leaf)?;
        validate_file(native.as_file(), &self.owner)?;
        if native.as_file().metadata()?.len() > config::MAX_IDENTITY_BYTES as u64 {
            return Err(Error::Identity("identity file is too large"));
        }
        let mut bytes = Zeroizing::new(Vec::new());
        native
            .as_file()
            .take((config::MAX_IDENTITY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > config::MAX_IDENTITY_BYTES {
            return Err(Error::Identity("identity file is too large"));
        }
        validate_file(native.as_file(), &self.owner)?;
        self.validate()?;
        Ok(bytes)
    }

    /// Publish complete nonsecret bytes atomically without replacing an existing
    /// name. The staging and final file remain owner-only; no pathname reopen or
    /// failed-operation cleanup deletes state. This is not a power-loss claim.
    pub fn publish_public(&self, leaf: &OsStr, bytes: &[u8]) -> Result<()> {
        use ring::rand::{SecureRandom, SystemRandom};
        if bytes.len() > config::MAX_IDENTITY_BYTES {
            return Err(Error::Identity("identity file is too large"));
        }
        validate_relative_component(leaf)?;
        // Dot names are reserved for staging, so a caller's final name can never
        // alias the newly created staging name (even under case-insensitive FS).
        if leaf.to_string_lossy().starts_with('.') {
            return Err(Error::Destination(
                "public name uses the reserved staging namespace",
            ));
        }
        let mut nonce = [0u8; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Configuration("publication randomness unavailable"))?;
        use std::fmt::Write as _;
        let mut stage = String::from(".kk-public-");
        for byte in nonce {
            write!(stage, "{byte:02x}").expect("string write");
        }
        let mut file = self.create_private_file(OsStr::new(&stage))?;
        file.write_bytes(bytes)?;
        self.validate()?;
        validate_file(file.native.as_file(), &self.owner)?;
        file.native.rename_no_replace(self.file.as_handle(), leaf)?;
        file.native.as_file().sync_all()?;
        validate_file(file.native.as_file(), &self.owner)?;
        self.validate()
    }

    /// Retained filesystem authority, not a promise about a separately supplied path.
    pub fn as_file(&self) -> &File {
        &self.file
    }
}
