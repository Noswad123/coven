// Resolve only the trusted fixture base, never a production root or a path
// containing symlinks deliberately introduced by a test.
pub(crate) fn tempdir() -> tempfile::TempDir {
    let base = std::env::temp_dir().canonicalize().unwrap();
    tempfile::tempdir_in(base).unwrap()
}
