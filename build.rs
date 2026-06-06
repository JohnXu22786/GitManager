use std::process::Command;

fn main() {
    // Capture git commit hash (if available)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Capture git describe (tag-based version, if available)
    let git_describe = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Build date (UTC)
    let build_date = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Date -Format 'yyyy-MM-ddTHH:mm:ssZ' -AsUTC"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("version_info.rs");

    std::fs::write(
        &dest_path,
        format!(
            r#"pub const VERSION: &str = "{}";
pub const GIT_HASH: &str = "{}";
pub const GIT_DESCRIBE: &str = "{}";
pub const BUILD_DATE: &str = "{}";
"#,
            env!("CARGO_PKG_VERSION"),
            git_hash,
            git_describe,
            build_date,
        ),
    )
    .unwrap();

    // Tell Cargo to rerun this script when git HEAD changes
    println!("cargo::rerun-if-changed=../.git/HEAD");
    // Also rerun if the version in Cargo.toml changes (Cargo does this automatically)
    println!("cargo::rerun-if-env-changed=GIT_MANAGER_VERSION");
}
