//! Compile-time release channel identity.
//!
//! Preview builds are compiled with `ABIGAIL_BUILD_PROFILE=preview`. A preview
//! install must never share an app identity, data root, OS credential, log or
//! runtime directory, Documents folder, or Hive port range with stable Abigail,
//! so a family can run both side by side without either touching the other.

use std::ffi::OsString;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/build_channel.rs"));

/// Every binary that links this crate contains exactly one
/// `abigail-build-channel=<name>;` marker. Packaging scripts search for it to
/// prove each executable in an installer was compiled for that channel.
pub const BUILD_MARKER_PREFIX: &str = "abigail-build-channel=";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Preview,
}

/// The channel this binary was compiled for.
pub fn current() -> Channel {
    // Reading through black_box keeps the marker bytes in every binary.
    Channel::from_build_marker(std::hint::black_box(BUILD_MARKER))
}

impl Channel {
    // Never spell out a whole marker here: only build.rs may emit one, or every
    // binary would also contain the other channel's marker.
    fn from_build_marker(marker: &str) -> Self {
        match marker.strip_prefix(BUILD_MARKER_PREFIX) {
            Some("preview;") => Self::Preview,
            _ => Self::Stable,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Preview => "preview",
        }
    }

    pub const fn product_name(self) -> &'static str {
        match self {
            Self::Stable => "Abigail",
            Self::Preview => "Abigail Preview",
        }
    }

    /// Folder under the local app-data directory holding logs and runtime
    /// descriptors. Preview keeps its data root here too, outside every stable
    /// path (Windows compares these names case-insensitively).
    pub const fn local_folder(self) -> &'static str {
        match self {
            Self::Stable => "Abigail",
            Self::Preview => "AbigailPreview",
        }
    }

    /// Hidden folder under `$HOME` for logs when no local app-data directory exists.
    pub const fn home_folder(self) -> &'static str {
        match self {
            Self::Stable => ".abigail",
            Self::Preview => ".abigail-preview",
        }
    }

    /// Folder under the user's Documents for Entity documents, archives, and backups.
    pub const fn documents_folder(self) -> &'static str {
        match self {
            Self::Stable => "Abigail",
            Self::Preview => "Abigail Preview",
        }
    }

    /// OS credential-store service that holds the vault root key. Preview's
    /// name must not contain `com.abigail.preview`: packaging checks that only
    /// preview app binaries embed that Tauri identifier.
    pub const fn keyring_service(self) -> &'static str {
        match self {
            Self::Stable => "com.abigail.vault",
            Self::Preview => "com.abigail.vault.preview",
        }
    }

    /// Loopback ports a desktop Hive daemon may bind.
    pub const fn hive_ports(self) -> RangeInclusive<u16> {
        match self {
            Self::Stable => 43141..=43150,
            Self::Preview => 43151..=43160,
        }
    }

    pub fn default_hive_url(self) -> String {
        format!("http://127.0.0.1:{}", self.hive_ports().start())
    }

    /// Default durable data root (`memory.db`, vault, identities).
    pub fn default_data_root(self) -> Option<PathBuf> {
        self.data_root_in(std::env::var_os("LOCALAPPDATA"))
    }

    fn data_root_in(self, local_app_data: Option<OsString>) -> Option<PathBuf> {
        match self {
            // Existing stable installs keep their data where it already is.
            Self::Stable => project_data_dir("Abigail"),
            Self::Preview => nonempty(local_app_data)
                .map(|base| base.join(self.local_folder()).join("data"))
                .or_else(|| project_data_dir("AbigailPreview")),
        }
    }

    /// Durable log directory. Resolved from environment variables only, so a
    /// failing platform directory lookup still leaves a readable trail.
    pub fn log_dir(self) -> PathBuf {
        self.log_dir_in(std::env::var_os("LOCALAPPDATA"), std::env::var_os("HOME"))
    }

    fn log_dir_in(self, local_app_data: Option<OsString>, home: Option<OsString>) -> PathBuf {
        if let Some(base) = nonempty(local_app_data) {
            return base.join(self.local_folder()).join("logs");
        }
        if let Some(home) = nonempty(home) {
            return home.join(self.home_folder()).join("logs");
        }
        std::env::temp_dir().join(self.local_folder()).join("logs")
    }

    /// Directory for the desktop Hive runtime descriptor.
    pub fn runtime_dir(self) -> PathBuf {
        self.runtime_dir_in(std::env::var_os("LOCALAPPDATA"))
    }

    fn runtime_dir_in(self, local_app_data: Option<OsString>) -> PathBuf {
        nonempty(local_app_data)
            .unwrap_or_else(std::env::temp_dir)
            .join(self.local_folder())
            .join("runtime")
    }

    /// `<documents>/<channel documents folder>`.
    pub fn documents_dir(self, documents: &Path) -> PathBuf {
        documents.join(self.documents_folder())
    }
}

fn project_data_dir(application: &str) -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "abigail", application)
        .map(|dirs| dirs.data_local_dir().to_path_buf())
}

fn nonempty(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNELS: [Channel; 2] = [Channel::Stable, Channel::Preview];

    fn local() -> Option<OsString> {
        Some(OsString::from("/fixture/LocalAppData"))
    }

    fn lowercase_components(path: &Path) -> Vec<String> {
        path.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    }

    /// Windows paths are case-insensitive, so `Abigail` and `abigail` are one folder.
    fn nested_case_insensitively(inner: &Path, outer: &Path) -> bool {
        let inner = lowercase_components(inner);
        let outer = lowercase_components(outer);
        inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
    }

    #[test]
    fn current_matches_the_compiled_build_profile() {
        let expected = match option_env!("ABIGAIL_BUILD_PROFILE") {
            Some("preview") => Channel::Preview,
            _ => Channel::Stable,
        };
        assert_eq!(current(), expected);
    }

    fn marker_for(channel: Channel) -> String {
        format!("{BUILD_MARKER_PREFIX}{};", channel.name())
    }

    #[test]
    fn compiled_marker_names_exactly_one_channel() {
        assert_eq!(BUILD_MARKER, marker_for(current()));
        for channel in CHANNELS {
            assert_eq!(Channel::from_build_marker(&marker_for(channel)), channel);
        }
        assert_eq!(
            Channel::from_build_marker("abigail-build-channel=previewx;"),
            Channel::Stable
        );
    }

    #[test]
    fn stable_identity_is_unchanged_for_existing_installs() {
        let stable = Channel::Stable;
        assert_eq!(stable.product_name(), "Abigail");
        assert_eq!(stable.keyring_service(), "com.abigail.vault");
        assert_eq!(stable.hive_ports(), 43141..=43150);
        assert_eq!(stable.default_hive_url(), "http://127.0.0.1:43141");
        assert_eq!(
            stable.data_root_in(local()),
            directories::ProjectDirs::from("com", "abigail", "Abigail")
                .map(|d| d.data_local_dir().to_path_buf())
        );
        assert_eq!(
            stable.log_dir_in(local(), None),
            PathBuf::from("/fixture/LocalAppData/Abigail/logs")
        );
        assert_eq!(
            stable.log_dir_in(None, Some("/home/family".into())),
            PathBuf::from("/home/family/.abigail/logs")
        );
        assert_eq!(
            stable.runtime_dir_in(local()),
            PathBuf::from("/fixture/LocalAppData/Abigail/runtime")
        );
        assert_eq!(
            stable.documents_dir(Path::new("/Docs")),
            PathBuf::from("/Docs/Abigail")
        );
    }

    #[test]
    fn preview_keeps_all_local_state_in_one_separate_folder() {
        let preview = Channel::Preview;
        let root = PathBuf::from("/fixture/LocalAppData/AbigailPreview");
        assert_eq!(preview.data_root_in(local()), Some(root.join("data")));
        assert_eq!(preview.log_dir_in(local(), None), root.join("logs"));
        assert_eq!(preview.runtime_dir_in(local()), root.join("runtime"));
    }

    #[test]
    fn preview_never_shares_a_stable_identity_or_resource() {
        let (stable, preview) = (Channel::Stable, Channel::Preview);
        assert_ne!(stable.product_name(), preview.product_name());
        assert_ne!(stable.keyring_service(), preview.keyring_service());
        assert_ne!(
            stable.documents_folder().to_lowercase(),
            preview.documents_folder().to_lowercase()
        );
        assert!(
            stable.hive_ports().end() < preview.hive_ports().start(),
            "Hive port ranges must not overlap"
        );
        assert_ne!(stable.data_root_in(None), preview.data_root_in(None));
        assert_ne!(stable.data_root_in(local()), preview.data_root_in(local()));
        assert_ne!(
            stable.log_dir_in(None, Some("/home/family".into())),
            preview.log_dir_in(None, Some("/home/family".into()))
        );
    }

    #[test]
    fn preview_state_is_never_inside_a_stable_folder_on_case_insensitive_disks() {
        let stable_roots = [
            PathBuf::from("/fixture/LocalAppData").join(Channel::Stable.local_folder()),
            // directories' Windows layout for the stable data root.
            PathBuf::from("/fixture/LocalAppData/abigail/Abigail"),
        ];
        let preview = Channel::Preview;
        let preview_paths = [
            preview.data_root_in(local()).unwrap(),
            preview.log_dir_in(local(), None),
            preview.runtime_dir_in(local()),
        ];
        for stable_root in &stable_roots {
            for path in &preview_paths {
                assert!(
                    !nested_case_insensitively(path, stable_root),
                    "{} is inside stable {}",
                    path.display(),
                    stable_root.display()
                );
            }
        }
    }

    #[test]
    fn blank_environment_values_fall_back_instead_of_using_relative_paths() {
        for channel in CHANNELS {
            let blank = Some(OsString::from("  "));
            assert!(channel
                .log_dir_in(blank.clone(), blank.clone())
                .is_absolute());
            assert!(channel.runtime_dir_in(blank.clone()).is_absolute());
            assert_eq!(channel.data_root_in(blank), channel.data_root_in(None));
        }
    }

    #[test]
    fn every_hive_port_range_is_bounded_and_ordered() {
        for channel in CHANNELS {
            let ports = channel.hive_ports();
            assert!(ports.start() < ports.end());
            assert_eq!(ports.clone().count(), 10);
            assert!(channel
                .default_hive_url()
                .ends_with(&format!(":{}", ports.start())));
        }
    }
}
