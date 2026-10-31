#[path = "../../src/test_support.rs"]
mod roots;

pub fn private_root() -> tempfile::TempDir {
    roots::tempdir().unwrap()
}
