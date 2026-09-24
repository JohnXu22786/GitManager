pub fn generate(version: &str, git_hash: &str, git_describe: &str, build_date: &str) -> String {
    format!(
        r#"pub const VERSION: &str = {:?};
pub const GIT_HASH: &str = {:?};
pub const GIT_DESCRIBE: &str = {:?};
pub const BUILD_DATE: &str = {:?};
"#,
        version, git_hash, git_describe, build_date,
    )
}
