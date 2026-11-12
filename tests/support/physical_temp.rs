pub fn tempdir() -> tempfile::TempDir {
    // Resolve the known-good base, never a fixture's intentional symlink aliases.
    let base = std::env::temp_dir().canonicalize().unwrap();
    tempfile::tempdir_in(base).unwrap()
}
