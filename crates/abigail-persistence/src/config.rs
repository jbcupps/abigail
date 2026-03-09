#[cfg(test)]
pub const TEST_MODE: bool = true;
#[cfg(not(test))]
pub const TEST_MODE: bool = false;

pub fn ci_mode_enabled() -> bool {
    std::env::var_os("ABIGAIL_CI_MODE").is_some()
}
