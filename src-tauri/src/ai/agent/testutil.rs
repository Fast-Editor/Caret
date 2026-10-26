//! Test helpers: an isolated CARET_HOME shared by the whole test binary.
use once_cell::sync::Lazy;
use std::path::PathBuf;

static HOME: Lazy<PathBuf> = Lazy::new(|| {
    let dir = std::env::temp_dir().join(format!("caret-test-home-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("CARET_HOME", &dir);
    dir
});

pub fn test_home() -> PathBuf { HOME.clone() }

pub fn temp_workspace() -> tempfile::TempDir {
    test_home();
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("main.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
    std::fs::create_dir_all(d.path().join("src")).unwrap();
    std::fs::write(d.path().join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
    d
}
