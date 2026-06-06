use std::process::Command;
use std::time::SystemTime;

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

    // Build date (UTC) — cross-platform using SystemTime
    let build_date = format_build_date().unwrap_or_else(|| "unknown".to_string());

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

/// Returns the current UTC time as an ISO 8601 formatted string.
/// Uses only stdlib types — no external command execution.
fn format_build_date() -> Option<String> {
    let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let total_secs = dur.as_secs();

    let secs_in_day = total_secs % 86400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;

    // Days since Unix epoch
    let z = (total_secs / 86400) + 719468;

    // Civil date from days (algorithm by Howard Hinnant)
    let era = (z as i64) / 146097i64;
    let doe = (z as i64) - era * 146097i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hour, minute, second
    ))
}
