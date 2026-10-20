//! Cooperative process-lifetime exclusion for cold preview profile operations.
//!
//! Every current preview app/daemon holds a shared lease until process exit.
//! A cold operation requires both exclusive participation and Hive store locks.
//! Lock files and PIDs are never evidence that a process has stopped: only the
//! acquired OS locks grant exclusion. This cannot account for older binaries
//! that do not participate, or arbitrary external programs modifying the files.

use crate::profile::ProfileContext;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};

const USE_LOCK: &str = "profile-use.lock";
const HIVE_LOCK: &str = "hive-owner.lock";

/// Held by `initialize_process` for the entire process lifetime, including
/// WebView shutdown and orphaned Entity-daemon lifetime after a Hive crash.
#[derive(Debug)]
pub struct ProfileUseLease {
    _file: File,
}

impl ProfileUseLease {
    pub fn acquire(profile: &ProfileContext) -> Result<Self> {
        let file = open_lock(profile, USE_LOCK)?;
        file.try_lock_shared().context(
            "Preview profile is undergoing a cold snapshot/restore; wait for it to finish before opening Abigail",
        )?;
        Ok(Self { _file: file })
    }
}

/// Keep this guard alive for the entire snapshot operation and its validation.
/// The existing Hive lock also excludes a Hive owner predating participation.
/// This is not a shutdown API and never kills processes or trusts stale PIDs.
#[derive(Debug)]
pub struct ColdProfileLease {
    _use_file: File,
    _hive_file: File,
}

impl ColdProfileLease {
    pub fn acquire(profile: &ProfileContext) -> Result<Self> {
        let use_file = open_lock(profile, USE_LOCK)?;
        use_file.try_lock().context(
            "Preview profile is in use; close its Hive and Entity windows and stop its owned daemons before a cold snapshot/restore",
        )?;
        let hive_file = open_lock(profile, HIVE_LOCK)?;
        hive_file.try_lock().context(
            "Preview Hive still owns the store; confirm its owned shutdown and exit before a cold snapshot/restore",
        )?;
        Ok(Self {
            _use_file: use_file,
            _hive_file: hive_file,
        })
    }
}

fn open_lock(profile: &ProfileContext, name: &str) -> Result<File> {
    let directory = profile.runtime_dir();
    profile.validate_path(&directory)?;
    std::fs::create_dir_all(&directory)
        .context("Cannot prepare preview lifecycle lock directory")?;
    let path = directory.join(name);
    profile.validate_path(&path)?;
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .context("Cannot open preview lifecycle lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{self, new_instance_id};
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    struct Fixture {
        container: PathBuf,
        profile: ProfileContext,
    }

    impl Fixture {
        fn new() -> Self {
            let container =
                std::env::temp_dir().join(format!("preview-lease-{}", new_instance_id()));
            fs::create_dir_all(container.join("child-temp")).unwrap();
            fs::write(container.join("fixture-gate"), b"cold-profile-lease-v1").unwrap();
            let profile = ProfileContext::preflight(&container.join("ongoing"), &[]).unwrap();
            Self { container, profile }
        }

        fn child(&self, mode: &str) -> ChildGuard {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "profile_lease::tests::process_lease_child_fixture",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env_clear()
                .env("ABIGAIL_PROFILE", "preview")
                .env("ABIGAIL_DATA_DIR", self.profile.root())
                .env("ABIGAIL_PROFILE_ID", self.profile.profile_id())
                .env("ABIGAIL_LEASE_FIXTURE", &self.container)
                .env("ABIGAIL_LEASE_FIXTURE_MODE", mode)
                .env("TEMP", self.container.join("child-temp"))
                .env("TMP", self.container.join("child-temp"))
                .env("TMPDIR", self.container.join("child-temp"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit());
            for name in ["SystemRoot", "WINDIR"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            ChildGuard(command.spawn().unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.container);
        }
    }

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    #[test]
    fn existing_inspection_never_initializes_an_absent_marker() {
        let fixture = Fixture::new();
        let absent = fixture.container.join("absent");
        assert!(ProfileContext::inspect_existing(&absent, &[]).is_err());
        assert!(!absent.exists());
        let empty = fixture.container.join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(ProfileContext::inspect_existing(&empty, &[]).is_err());
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
        let before = fs::read(fixture.profile.root().join("profile.json")).unwrap();
        let inspected = ProfileContext::inspect_existing(fixture.profile.root(), &[]).unwrap();
        assert_eq!(inspected.profile_id(), fixture.profile.profile_id());
        assert_eq!(
            fs::read(fixture.profile.root().join("profile.json")).unwrap(),
            before
        );
        assert!(!fixture.profile.runtime_dir().exists());
    }

    #[test]
    fn cold_ownership_inspection_allows_damaged_configs_without_modifying_them() {
        let fixture = Fixture::new();
        let marker_path = fixture.profile.root().join("profile.json");
        let config_path = fixture.profile.root().join("config.json");
        let marker_before = fs::read(&marker_path).unwrap();
        let damaged_config = b"{ damaged original config, retained for recovery";
        fs::write(&config_path, damaged_config).unwrap();
        let inspected = ProfileContext::inspect_for_cold_ownership(
            fixture.profile.root(),
            &[],
            fixture.profile.profile_id(),
        )
        .unwrap();
        assert_eq!(inspected.profile_id(), fixture.profile.profile_id());
        assert!(ProfileContext::inspect_existing(fixture.profile.root(), &[]).is_err());
        assert_eq!(fs::read(marker_path).unwrap(), marker_before);
        assert_eq!(fs::read(config_path).unwrap(), damaged_config);
        assert_eq!(fs::read_dir(fixture.profile.root()).unwrap().count(), 2);
    }

    #[test]
    fn cold_ownership_inspection_requires_expected_existing_compatible_marker() {
        let fixture = Fixture::new();
        let marker_path = fixture.profile.root().join("profile.json");
        let marker_before = fs::read(&marker_path).unwrap();
        let wrong_id = new_instance_id();
        assert!(
            ProfileContext::inspect_for_cold_ownership(fixture.profile.root(), &[], &wrong_id,)
                .is_err()
        );
        assert!(ProfileContext::inspect_for_cold_ownership(
            fixture.profile.root(),
            &[fixture.profile.root().to_path_buf()],
            fixture.profile.profile_id(),
        )
        .is_err());
        let absent = fixture.container.join("absent");
        assert!(ProfileContext::inspect_for_cold_ownership(
            &absent,
            &[],
            fixture.profile.profile_id(),
        )
        .is_err());
        assert!(!absent.exists());
        assert_eq!(fs::read(&marker_path).unwrap(), marker_before);
        assert_eq!(fs::read_dir(fixture.profile.root()).unwrap().count(), 1);

        let mut marker: serde_json::Value = serde_json::from_slice(&marker_before).unwrap();
        marker["schema_version"] = (profile::PROFILE_SCHEMA_VERSION + 1).into();
        let future = serde_json::to_vec_pretty(&marker).unwrap();
        fs::write(&marker_path, &future).unwrap();
        assert!(ProfileContext::inspect_for_cold_ownership(
            fixture.profile.root(),
            &[],
            fixture.profile.profile_id(),
        )
        .is_err());
        assert_eq!(fs::read(&marker_path).unwrap(), future);
        assert_eq!(fs::read_dir(fixture.profile.root()).unwrap().count(), 1);
    }

    #[test]
    fn shared_users_exclude_cold_operations_until_every_user_exits() {
        let fixture = Fixture::new();
        let first = ProfileUseLease::acquire(&fixture.profile).unwrap();
        let second = ProfileUseLease::acquire(&fixture.profile).unwrap();
        assert!(ColdProfileLease::acquire(&fixture.profile).is_err());
        drop(first);
        assert!(ColdProfileLease::acquire(&fixture.profile).is_err());
        drop(second);
        let cold = ColdProfileLease::acquire(&fixture.profile).unwrap();
        assert!(ProfileUseLease::acquire(&fixture.profile).is_err());
        assert!(ColdProfileLease::acquire(&fixture.profile).is_err());
        drop(cold);
        assert!(ProfileUseLease::acquire(&fixture.profile).is_ok());
    }

    #[test]
    fn legacy_hive_lock_blocks_cold_operation_without_leaking_participation_lock() {
        let fixture = Fixture::new();
        let hive = open_lock(&fixture.profile, HIVE_LOCK).unwrap();
        hive.try_lock().unwrap();
        let error = ColdProfileLease::acquire(&fixture.profile).unwrap_err();
        assert!(format!("{error:#}").contains("Hive still owns"));
        assert!(ProfileUseLease::acquire(&fixture.profile).is_ok());
        drop(hive);
        assert!(ColdProfileLease::acquire(&fixture.profile).is_ok());
    }

    #[test]
    fn stale_lock_contents_are_preserved_and_never_treated_as_ownership() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.profile.runtime_dir()).unwrap();
        for name in [USE_LOCK, HIVE_LOCK] {
            fs::write(
                fixture.profile.runtime_dir().join(name),
                b"stale PID is not authority",
            )
            .unwrap();
        }
        drop(ColdProfileLease::acquire(&fixture.profile).unwrap());
        for name in [USE_LOCK, HIVE_LOCK] {
            assert_eq!(
                fs::read(fixture.profile.runtime_dir().join(name)).unwrap(),
                b"stale PID is not authority"
            );
        }
    }

    #[test]
    fn cold_operation_refuses_fresh_process_startup() {
        let fixture = Fixture::new();
        let cold = ColdProfileLease::acquire(&fixture.profile).unwrap();
        let mut child = fixture.child("refused");
        assert!(child.0.wait().unwrap().success());
        assert!(!fixture.profile.logs_dir().exists());
        assert!(!fixture.profile.shared_memory_path().exists());
        drop(cold);
        assert!(ProfileUseLease::acquire(&fixture.profile).is_ok());
    }

    #[test]
    fn initialized_process_keeps_lease_after_local_context_is_dropped() {
        let fixture = Fixture::new();
        let mut child = fixture.child("hold");
        let ready = fixture.container.join("child-ready");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "Fixture exited early"
            );
            assert!(Instant::now() < deadline, "Fixture did not initialize");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ColdProfileLease::acquire(&fixture.profile).is_err());
        fs::write(fixture.container.join("child-release"), b"exit").unwrap();
        assert!(child.0.wait().unwrap().success());
        assert!(ColdProfileLease::acquire(&fixture.profile).is_ok());
    }

    #[test]
    fn interrupted_staging_is_inspectable_but_fresh_process_startup_refuses_it() {
        let mut fixture = Fixture::new();
        let stage = fixture
            .container
            .join(format!(".abigail-cold-{}.partial", new_instance_id()));
        fixture.profile = ProfileContext::preflight(&stage, &[]).unwrap();
        assert!(ProfileContext::inspect_existing(&stage, &[]).is_ok());
        let marker = fs::read(stage.join("profile.json")).unwrap();
        let mut child = fixture.child("staging");
        assert!(child.0.wait().unwrap().success());
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 1);
        assert_eq!(fs::read(stage.join("profile.json")).unwrap(), marker);
        assert!(profile::reject_cold_staging_root(&stage.join("profile")).is_err());
        assert!(profile::reject_cold_staging_root(
            &fixture.container.join(".ABIGAIL-COLD-example.PARTIAL")
        )
        .is_err());
        assert!(profile::reject_cold_staging_root(&fixture.container.join("ongoing")).is_ok());
    }

    #[test]
    #[ignore = "Only synthetic lease parent tests may invoke this child fixture"]
    fn process_lease_child_fixture() {
        let container = PathBuf::from(std::env::var_os("ABIGAIL_LEASE_FIXTURE").unwrap());
        assert!(container.is_absolute());
        assert!(container
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("preview-lease-"));
        assert_eq!(
            fs::read(container.join("fixture-gate")).unwrap(),
            b"cold-profile-lease-v1"
        );
        let mode = std::env::var("ABIGAIL_LEASE_FIXTURE_MODE").unwrap();
        let root = PathBuf::from(std::env::var_os("ABIGAIL_DATA_DIR").unwrap());
        assert!(profile::same_path(
            root.parent().unwrap(),
            &profile::canonical_without_links(&container).unwrap(),
        ));
        if mode == "staging" {
            assert!(profile::reject_cold_staging_root(&root).is_err());
        } else {
            assert_eq!(root.file_name().unwrap(), "ongoing");
        }
        assert!(profile::same_path(
            &profile::canonical_without_links(&root).unwrap(),
            &PathBuf::from(std::env::var_os("ABIGAIL_DATA_DIR").unwrap()),
        ));
        let initialized = profile::initialize_process(Some("preview"), Some(&root));
        if mode == "staging" {
            let error = initialized.unwrap_err();
            assert!(format!("{error:#}").contains("staging cannot be opened"));
            assert!(profile::current().is_err());
            return;
        }
        if mode == "refused" {
            let error = initialized.unwrap_err();
            assert!(format!("{error:#}").contains("cold snapshot/restore"));
            assert!(profile::current().is_err());
            return;
        }
        assert_eq!(mode, "hold");
        drop(initialized.unwrap().unwrap());
        assert!(profile::current().unwrap().is_some());
        fs::write(container.join("child-ready"), b"initialized").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !container.join("child-release").exists() {
            assert!(
                Instant::now() < deadline,
                "Fixture release was not requested"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
