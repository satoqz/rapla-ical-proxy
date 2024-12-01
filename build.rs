use std::process::Command;

fn main() {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_commit_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_COMMIT_HASH={git_commit_hash}");
}
