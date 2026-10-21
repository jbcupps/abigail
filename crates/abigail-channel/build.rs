// A misspelled profile must not silently produce a stable build that would
// read and write the family's stable data.
fn main() {
    println!("cargo:rerun-if-env-changed=ABIGAIL_BUILD_PROFILE");
    let channel = match std::env::var("ABIGAIL_BUILD_PROFILE") {
        Err(std::env::VarError::NotPresent) => "stable",
        Ok(value) => match value.as_str() {
            "" | "stable" => "stable",
            "preview" => "preview",
            other => panic!("ABIGAIL_BUILD_PROFILE must be 'stable' or 'preview', not '{other}'"),
        },
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!("ABIGAIL_BUILD_PROFILE must be 'stable' or 'preview'")
        }
    };
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        out_dir.join("build_channel.rs"),
        format!("const BUILD_MARKER: &str = \"abigail-build-channel={channel};\";\n"),
    )
    .expect("write build channel marker");
}
