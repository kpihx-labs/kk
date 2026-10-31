# Changelog

## [0.5.1](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.5.0...windows-namespace-request-sys-v0.5.1) (2026-10-22)


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.3.2 to 0.3.3

## [0.5.0](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.4.2...windows-namespace-request-sys-v0.5.0) (2026-10-20)


### ⚠ BREAKING CHANGES

* **namespace-request:** delete handle capture; every request shares its handle
* **namespace-request:** every entry that takes a handle holds a SharedHandle

### Features

* **namespace-request:** add QueryFileBasicInfo, a typed read of FileBasicInfo ([40d9f92](https://github.com/MikeGrier/windows-threadpool-sys/commit/40d9f92869930f434bd37d76fb41996229c5c915))
* **namespace-request:** add QueryFileIdInfo, FileIdInfo read as a volume serial and a 128-bit file ID ([24f6ebd](https://github.com/MikeGrier/windows-threadpool-sys/commit/24f6ebd40b29f8547c13319cf7907492733fe1f1))
* **namespace-request:** add QueryFileType, the GetFileType entry ([24f6ebd](https://github.com/MikeGrier/windows-threadpool-sys/commit/24f6ebd40b29f8547c13319cf7907492733fe1f1))
* **namespace-request:** build OpenFile and WatchDirectory from an AnchoredPath ([40d9f92](https://github.com/MikeGrier/windows-threadpool-sys/commit/40d9f92869930f434bd37d76fb41996229c5c915))
* **namespace-request:** delete handle capture; every request shares its handle ([40d9f92](https://github.com/MikeGrier/windows-threadpool-sys/commit/40d9f92869930f434bd37d76fb41996229c5c915))
* **namespace-request:** every entry that takes a handle holds a SharedHandle ([24f6ebd](https://github.com/MikeGrier/windows-threadpool-sys/commit/24f6ebd40b29f8547c13319cf7907492733fe1f1))
* **namespace-request:** make AnchoredPath public, so a crate building its own request takes the path contract from here ([24f6ebd](https://github.com/MikeGrier/windows-threadpool-sys/commit/24f6ebd40b29f8547c13319cf7907492733fe1f1))
* **namespace-request:** QueryFileInformation::perform_into, into a caller's buffer, refusing an address off the boundary a structure's page requires (PerformIntoError) ([40d9f92](https://github.com/MikeGrier/windows-threadpool-sys/commit/40d9f92869930f434bd37d76fb41996229c5c915))

## [0.4.2](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.4.1...windows-namespace-request-sys-v0.4.2) (2026-10-17)


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.3.1 to 0.3.2

## [0.4.1](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.4.0...windows-namespace-request-sys-v0.4.1) (2026-10-17)


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.3.0 to 0.3.1

## [0.4.0](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.3.1...windows-namespace-request-sys-v0.4.0) (2026-10-16)

Corrected after the release: the generated list named six storage and volume entries (`QueryStorageProperty`,
`QueryStorageDeviceNumaNode`, `QueryStorageDeviceUniqueId`, `GetStorageDependencyInformation`,
`QueryVolumeDiskExtents`, `QueryVolumeNumaInfo`) and the `DeviceIoControl` helper under them. They were
moved to `win-volume-sys` before this version was published, so none of them is in this crate's 0.4.0
and the lines are removed.

### ⚠ BREAKING CHANGES

* **namespace-request:** OpenFile::new, WatchDirectory::new and ResolveFullPath::new take a Wtf16String and return a Result; prepare, PreparedPath, PathError, PathFailure and the path module are removed.

### Features

* **namespace-request:** add the ReOpenFile entry ([b558570](https://github.com/MikeGrier/windows-threadpool-sys/commit/b55857076135ab41a4985680517b4c49b8218b8d))
* **namespace-request:** open, watch and resolve by a Wtf16String, and retire prepare ([3083c05](https://github.com/MikeGrier/windows-threadpool-sys/commit/3083c050319c9bbee663c1f784ebbdd6084c2be2))
* **namespace-request:** SecurityDescriptor::from_sddl and to_sddl ([d618a6f](https://github.com/MikeGrier/windows-threadpool-sys/commit/d618a6f900c84d0d912624797e0040758c4b82e2))


### Bug Fixes

* **namespace-request:** do not graft a child onto a verbatim or device current directory ([7e95094](https://github.com/MikeGrier/windows-threadpool-sys/commit/7e95094af63cb5759fd7e24d584cee033f7c9e57))
* **namespace-request:** Win32Error is win_error_codes::Win32Error, which keeps a code of 2^31 or more ([0f9474c](https://github.com/MikeGrier/windows-threadpool-sys/commit/0f9474c15e61325cd6c605ae6157c031f62947a3))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * win-error-codes bumped from 0.1.0 to 0.0.2

## [0.3.1](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.3.0...windows-namespace-request-sys-v0.3.1) (2026-10-15)


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.2.3 to 0.3.0

## [0.3.0](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.2.3...windows-namespace-request-sys-v0.3.0) (2026-10-15)


### ⚠ BREAKING CHANGES

* **namespace-request:** SecurityDescriptor and SecurityAttributes no longer implement Clone; use try_clone. QueryFileInformation::perform can return ERROR_NOT_ENOUGH_MEMORY and no longer panics.
* **namespace-request:** windows_namespace_request_sys::AlignedBuffer and the buffer module are removed. QueryFileInformation::perform and the Request output for it return AlignedBuf, whose constructors return a Result and which has try_clone in place of Clone.

### Bug Fixes

* **namespace-request:** report an allocation that fails as an error ([aab545c](https://github.com/MikeGrier/windows-threadpool-sys/commit/aab545cf9395c8e3ddb2ac8bb4b3792575fd5ab1))


### Code Refactoring

* **namespace-request:** replace AlignedBuffer with io-buffers' AlignedBuf ([b61de26](https://github.com/MikeGrier/windows-threadpool-sys/commit/b61de26d0e8478f28d39234d0e4ed02631fdb4d8))


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.2.2 to 0.2.3

## [0.2.3](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.2.2...windows-namespace-request-sys-v0.2.3) (2026-10-04)


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.2.1 to 0.2.2

## [0.2.2](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.2.1...windows-namespace-request-sys-v0.2.2) (2026-09-25)


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.2.0 to 0.2.1

## [0.2.1](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.2.0...windows-namespace-request-sys-v0.2.1) (2026-09-10)


### Bug Fixes

* **namespace-request:** assert which device each name reaches, and repoint the links the split moved ([dbbecce](https://github.com/MikeGrier/windows-threadpool-sys/commit/dbbeccefedb209cb7702dd44c376200bdbcbcd9d))
* **namespace-request:** grow the entry buffer, and stop the probe contradicting itself ([092bda8](https://github.com/MikeGrier/windows-threadpool-sys/commit/092bda82dfe7a8b1adeff781b640866edbbcd2f0))
* **namespace-request:** NUL is the one device word a path in front of it does not save ([4198862](https://github.com/MikeGrier/windows-threadpool-sys/commit/41988625eba4ace6ef31b9a9143d985753e97a88))
* **namespace-request:** state the current-drive arm at the boundary observation reaches ([e13aac7](https://github.com/MikeGrier/windows-threadpool-sys/commit/e13aac78d4789c88d2ff2cfc88a98307ec55858b))
* **namespace-request:** take the handle-allocation guard, and say which layer the device rules describe ([3e48630](https://github.com/MikeGrier/windows-threadpool-sys/commit/3e48630b1cfd47a2037f2393fbd05f50334099ed))
* **namespace-request:** tell an empty per-drive entry from an absent one ([016f032](https://github.com/MikeGrier/windows-threadpool-sys/commit/016f0320ff6a46ebd01a4f1e88818f3908662c74))
* **namespace-request:** the lexical half claimed a mechanism too, for twenty-three rounds ([c20ef91](https://github.com/MikeGrier/windows-threadpool-sys/commit/c20ef91c534ac4c24101165c49344584d6470c4e))

## [0.2.0](https://github.com/MikeGrier/windows-threadpool-sys/compare/windows-namespace-request-sys-v0.1.0...windows-namespace-request-sys-v0.2.0) (2026-08-29)


### ⚠ BREAKING CHANGES

* **namespace-request:** `ResolveFullPath::perform` returns `Result<Wtf16String, FullPathError>` rather than `Outcome<Wtf16String>`, and its `Request::Error` is `FullPathError` rather than `Win32Error`. A caller that only used `?` into a boxed error is unaffected; one that matched `Win32Error` directly now matches `FullPathError::Win32`. The crate is unpublished, so no released version is affected.

### Features

* **namespace-request:** add path preparation, copied from the enumeration crate ([e843dae](https://github.com/MikeGrier/windows-threadpool-sys/commit/e843dae4292c0d1ee5970b96ae514a5921be89ab))
* **namespace-request:** add the close entries, with the routine carried by the handle ([267c13a](https://github.com/MikeGrier/windows-threadpool-sys/commit/267c13ab29f29c6997760058dd59d1173c3d7634))
* **namespace-request:** add the CreateFileW entry ([4239bab](https://github.com/MikeGrier/windows-threadpool-sys/commit/4239bab26bd7d60ee7c7c9c677c8f1a3dfa039b5))
* **namespace-request:** add the FindFirstChangeNotificationW entry ([0d53b5f](https://github.com/MikeGrier/windows-threadpool-sys/commit/0d53b5f7b710c09f4dc170eebe42c18a27065db8))
* **namespace-request:** add the GetFileInformationByHandle entry ([17c67fe](https://github.com/MikeGrier/windows-threadpool-sys/commit/17c67fe119f20131401ebabf3e24fe445b00d745))
* **namespace-request:** add the GetFileInformationByHandleEx entry ([af89761](https://github.com/MikeGrier/windows-threadpool-sys/commit/af897618e6d59bb775ec0a3857dfdd596a8b7a67))
* **namespace-request:** add the GetFinalPathNameByHandleW entry ([887c856](https://github.com/MikeGrier/windows-threadpool-sys/commit/887c85684bc3e55a12e9493f97934954b4dc6913))
* **namespace-request:** add the OpenFileById entry ([1f5912f](https://github.com/MikeGrier/windows-threadpool-sys/commit/1f5912f1ff4166bc7e6db2dc80944ac0326cd585))
* **namespace-request:** add the volume and full-path entries ([4722145](https://github.com/MikeGrier/windows-threadpool-sys/commit/4722145d4a43c216d78c51f0d9681b54f98896b4))
* **namespace-request:** capture handles as owned duplicates ([5b0ca31](https://github.com/MikeGrier/windows-threadpool-sys/commit/5b0ca31ec460c7589e4ac95c4c3210ba334bacea))
* **namespace-request:** capture security attributes as an owned self-relative blob ([126eb5f](https://github.com/MikeGrier/windows-threadpool-sys/commit/126eb5f46a9dfd669aab053f65a2cb612d30ecbe))
* **namespace-request:** create the crate and record its boundary decisions ([30b992d](https://github.com/MikeGrier/windows-threadpool-sys/commit/30b992dd54df8c9d8e5e7b6b07c606dcae143a0c))
* **namespace-request:** give the catalogue a test seam ([313a88f](https://github.com/MikeGrier/windows-threadpool-sys/commit/313a88f4e82f5b8402cea9f2521960cdadb0016c))
* **namespace-request:** make faithful execution a primitive, not a rule each entry restates ([8a03966](https://github.com/MikeGrier/windows-threadpool-sys/commit/8a03966086faa5b74017a4271dd16a4bfed347f3))


### Bug Fixes

* address the third review round ([344fed0](https://github.com/MikeGrier/windows-threadpool-sys/commit/344fed0289274b2cc224824a0186e725279c0d79))
* **namespace-request:** compare normalised paths, not temp_dir text ([1c83988](https://github.com/MikeGrier/windows-threadpool-sys/commit/1c83988faa9354195c601bc766e4133671df354b))
* **namespace-request:** give ResolveFullPath its own error instead of a synthesized code ([6e9b3f9](https://github.com/MikeGrier/windows-threadpool-sys/commit/6e9b3f9e0b308f49284aefd0947fd1298f219ad0))
* **namespace-request:** stop forming a slice over uninitialised buffer capacity ([c521a1b](https://github.com/MikeGrier/windows-threadpool-sys/commit/c521a1b3fa9be2c8f14a3c85cb5dd6abbe1f511d))


### Dependencies

* The following workspace dependencies were updated
  * dev-dependencies
    * windows-thread-ambient-sys bumped from 0.1.0 to 0.2.0

## Changelog

All notable changes to this project will be documented in this file. Releases
are generated by release-please from Conventional Commits.
