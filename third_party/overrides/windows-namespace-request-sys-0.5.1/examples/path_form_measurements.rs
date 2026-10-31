// Copyright (c) 2026 Mike Grier
//! How Win32 reads the prefix of a path, measured for `NR-5.1` and recorded in `D-28` of
//! `DESIGN-NOTES.md`. It prints a table; it asserts nothing, because what it shows is the answer of
//! the machine it runs on. Run it with `cargo run --example path_form_measurements`.
//!
//! Two questions are put to every spelling of a prefix, each through this crate's own entry for
//! the documented call:
//!
//! - what `GetFullPathNameW` makes of it ([`ResolveFullPath`]), which shows whether the string was
//!   normalised (a `.` component removed, a slash turned into a backslash) or left as it was; and
//! - what `CreateFileW` does with it ([`OpenFile`]), opening a file that exists with a `.`
//!   component in the middle of the path, or a share that does not exist, so that the outcome says
//!   whether the prefix was normalised and whether it was taken as a UNC path.
//!
//! The network cases name a share on `localhost` that does not exist, so no remote machine is
//! contacted. An open that was recognised as a UNC path fails with a network error, and one that was
//! not fails with a local one.

use windows_namespace_request_sys::{OpenFile, ResolveFullPath};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use wtf_string::Wtf16String;

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// A share on this machine that does not exist, so that nothing remote is contacted.
const NO_SUCH_SHARE: &str = r"localhost\wnrs-no-such-share\x";

/// The prefixes tried in front of a drive path: the verbatim and device forms with every mix of
/// slashes, and the NT form.
const DRIVE_PREFIXES: [&str; 14] = [
    r"\\?\", "//?/", r"\/?\", r"/\?\", r"\\?/", "//?\\", r"\\.\", "//./", r"\/.\", r"/\.\",
    r"\\./", "//.\\", r"\??\", "/??/",
];

/// The prefixes tried in front of `NO_SUCH_SHARE`. The last has no separator after `UNC`, so the
/// server name runs on from it: it asks whether `UNC` alone is a marker.
const UNC_PREFIXES: [&str; 13] = [
    r"\\",
    "//",
    r"\/",
    r"/\",
    r"\\?\UNC\",
    r"\\?\unc\",
    r"\\?\Unc\",
    "//?/UNC/",
    r"\\.\UNC\",
    r"\\.\unc\",
    "//./UNC/",
    r"\??\UNC\",
    r"\\?\UNC",
];

/// Strings that are only a prefix, or less, asked of `GetFullPathNameW` and never opened.
const EDGES: [&str; 17] = [
    "",
    r"\",
    r"\\",
    r"\\?",
    r"\\?\",
    r"\\.",
    r"\\.\",
    r"\\?\UNC",
    r"\\?\UNC\",
    r"\\.\UNC\",
    r"\??\",
    "C:",
    r"C:\",
    r"\\?\C:",
    r"\\?\C:\",
    r"\\?\Volume{00000000-0000-0000-0000-000000000000}\",
    r"\\localhost\",
];

fn resolved(path: &str) -> String {
    match ResolveFullPath::new(Wtf16String::from(path)) {
        Ok(request) => match request.perform() {
            Ok(full) => format!("{:?}", full.to_string_lossy()),
            Err(error) => format!("error: {error}"),
        },
        Err(error) => format!("not built: {error}"),
    }
}

fn opened(path: &str) -> String {
    let request = match OpenFile::new(Wtf16String::from(path)) {
        Ok(request) => request,
        Err(error) => return format!("not built: {error}"),
    };
    match request
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(SHARE_ALL)
        .with_creation_disposition(OPEN_EXISTING)
        .perform()
    {
        Ok(handle) => {
            drop(handle);
            "opened".to_owned()
        }
        Err(error) => format!("error {}", error.code()),
    }
}

fn row(label: &str, path: &str, open: bool) {
    let open = if open {
        opened(path)
    } else {
        "(not opened)".to_owned()
    };
    println!("{label:<30} {path:<48} {open:<14} {}", resolved(path));
}

fn main() {
    // `SystemRoot` is `C:\Windows` or the like; the drive path after the prefix is the same for
    // every row, and carries a `.` component that only a normalising reader removes.
    let root = std::env::var("SystemRoot").expect("Windows names its directory");
    let with_dot = format!(r"{root}\System32\.\kernel32.dll");
    let without_dot = format!(r"{root}\System32\kernel32.dll");

    println!("system root: {root}");
    println!(
        "{:<30} {:<48} {:<14} GetFullPathNameW",
        "row", "input", "CreateFileW"
    );
    println!();
    println!("-- controls: a drive path with and without the `.` component");
    row("plain, no dot", &without_dot, true);
    row("plain, dot", &with_dot, true);
    row("plain, dot, slashes", &with_dot.replace('\\', "/"), true);

    println!();
    println!("-- a drive path behind each prefix, with the `.` component");
    for prefix in DRIVE_PREFIXES {
        row(
            &format!("prefix {prefix}"),
            &format!("{prefix}{with_dot}"),
            true,
        );
    }

    println!();
    println!("-- a share that does not exist behind each prefix");
    row(
        "plain UNC with backslashes",
        &format!(r"\\{NO_SUCH_SHARE}"),
        true,
    );
    for prefix in UNC_PREFIXES {
        let tail = NO_SUCH_SHARE.replace('\\', if prefix.contains('/') { "/" } else { r"\" });
        row(
            &format!("prefix {prefix}"),
            &format!("{prefix}{tail}"),
            true,
        );
    }

    // A forward slash after `UNC`, behind each prefix: a verbatim path keeps a slash as a name
    // character, so it is the case where a marker could stop being one.
    println!();
    println!("-- a forward slash after UNC, and in the rest of the name, behind each prefix");
    for prefix in [r"\\?\UNC/", r"\\.\UNC/", r"\??\UNC/", r"//?/UNC\"] {
        let tail = NO_SUCH_SHARE.replace('\\', "/");
        row(
            &format!("prefix {prefix}"),
            &format!("{prefix}{tail}"),
            true,
        );
    }

    // The NT prefix: `GetFullPathNameW` reads it as a rooted path, and `CreateFileW` may not.
    // Without the `.` component the rows say whether the open treated it as an NT path.
    println!();
    println!("-- the NT prefix, without the `.` component, and a share behind it in each case");
    for prefix in [r"\??\", "/??/", r"\??/", r"/??\", r"\\??\", r"\\.\??\"] {
        let plain = if prefix.contains('/') {
            without_dot.replace('\\', "/")
        } else {
            without_dot.clone()
        };
        row(
            &format!("prefix {prefix}"),
            &format!("{prefix}{plain}"),
            true,
        );
    }
    for prefix in [r"\??\unc\", r"\??\UNC\", "/??/UNC/", r"\\??\UNC\"] {
        let tail = NO_SUCH_SHARE.replace('\\', if prefix.contains('/') { "/" } else { r"\" });
        row(
            &format!("prefix {prefix}"),
            &format!("{prefix}{tail}"),
            true,
        );
    }

    println!();
    println!("-- strings that are only a prefix or less: GetFullPathNameW alone");
    for edge in EDGES {
        row("edge", edge, false);
    }
}
