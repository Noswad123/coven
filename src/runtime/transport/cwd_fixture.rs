//! Inert executable for the isolated legacy caller-cwd regression only.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

fn main() {
    let home = PathBuf::from(std::env::var_os("HOME").expect("fixture HOME"));
    let executable = std::env::current_exe().expect("fixture executable");
    if executable
        .parent()
        .expect("executable parent")
        .join("decoy")
        .exists()
    {
        fs::write(home.join("decoy-executed"), b"wrong executable").expect("decoy observation");
        std::process::exit(0);
    }
    let mut calls = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("caller-calls"))
        .expect("caller observation");
    writeln!(
        calls,
        "{}",
        std::env::current_dir().expect("fixture cwd").display()
    )
    .expect("record cwd");
    std::process::exit(127);
}
