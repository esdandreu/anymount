// Copyright 2026 Dotphoton AG
#![allow(clippy::unwrap_used)]

#[cfg(target_os = "linux")]
use libc::kill;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::{Child, Command};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use tempfile::TempDir;

#[cfg(target_os = "linux")]
struct TestFixture {
    mount_path: PathBuf,
    _temp_dir: TempDir,
    child: Child,
}

#[cfg(target_os = "linux")]
impl TestFixture {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mount_path = temp_dir.path().join("mnt");
        let data_path = temp_dir.path().join("data");

        fs::create_dir(&mount_path).expect("Failed to create mnt dir");
        fs::create_dir(&data_path).expect("Failed to create data dir");

        fs::write(data_path.join("hello.txt"), "Hello, World!").expect("Failed to write hello.txt");

        let subdir = data_path.join("subdir");
        fs::create_dir(&subdir).expect("Failed to create subdir");
        fs::write(subdir.join("nested.txt"), "Nested content").expect("Failed to write nested.txt");

        let binary_path = env!("CARGO_BIN_EXE_anymount-cli");
        let child = Command::new(binary_path)
            .args([
                "connect-sync",
                "temp",
                mount_path.to_str().unwrap(),
                "local",
                data_path.to_str().unwrap(),
            ])
            .spawn()
            .expect("Failed to spawn provider");

        Self {
            mount_path,
            _temp_dir: temp_dir,
            child,
        }
    }

    fn wait_for_ready(&mut self) -> bool {
        let timeout = Duration::from_secs(5);
        let poll_interval = Duration::from_millis(50);
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                return false;
            }

            if let Some(_status) = self.child.try_wait().unwrap() {
                return false;
            }

            match fs::read_dir(&self.mount_path) {
                Ok(_) => return true,
                Err(_) => {}
            }

            std::thread::sleep(poll_interval);
        }
    }

    fn wait_for_file(&self, filename: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.mount_path.join(filename).exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn list_mount_contents(&self) -> Vec<String> {
        fs::read_dir(&self.mount_path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(target_os = "linux")]
impl Drop for TestFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn local_provider_connects_and_responds() {
    let mut fixture = TestFixture::new();
    assert!(
        fixture.wait_for_ready(),
        "Provider should connect and become ready"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn local_provider_lists_directory_contents() {
    let mut fixture = TestFixture::new();
    assert!(fixture.wait_for_ready(), "Provider should be ready");

    std::thread::sleep(Duration::from_millis(500));

    let entries = fixture.list_mount_contents();
    assert!(
        entries.contains(&"hello.txt".to_string()),
        "Mount should contain hello.txt, got: {:?}",
        entries
    );
    assert!(
        entries.contains(&"subdir".to_string()),
        "Mount should contain subdir, got: {:?}",
        entries
    );
}

#[cfg(target_os = "linux")]
#[test]
fn provider_cleans_up_on_drop() {
    let mut fixture = TestFixture::new();
    assert!(fixture.wait_for_ready(), "Provider should be ready");

    let child_id = fixture.child.id();
    drop(fixture);

    std::thread::sleep(Duration::from_millis(100));

    let result = unsafe { kill(child_id as libc::pid_t, 0) };
    assert_eq!(
        result, -1,
        "Child process {} should have been killed after fixture drop",
        child_id
    );
}

#[cfg(target_os = "linux")]
#[test]
fn provider_handles_empty_directory() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mount_path = temp_dir.path().join("mnt");
    let data_path = temp_dir.path().join("data");

    fs::create_dir(&mount_path).expect("Failed to create mnt dir");
    fs::create_dir(&data_path).expect("Failed to create data dir");

    let binary_path = env!("CARGO_BIN_EXE_anymount-cli");
    let mut child = Command::new(binary_path)
        .args([
            "connect-sync",
            "temp",
            mount_path.to_str().unwrap(),
            "local",
            data_path.to_str().unwrap(),
        ])
        .spawn()
        .expect("Failed to spawn provider");

    let timeout = Duration::from_secs(5);
    let poll_interval = Duration::from_millis(50);
    let start = Instant::now();
    let mut ready = false;

    loop {
        if start.elapsed() > timeout {
            break;
        }

        if let Some(_status) = child.try_wait().unwrap() {
            break;
        }

        match fs::read_dir(&mount_path) {
            Ok(_) => {
                ready = true;
                break;
            }
            Err(_) => {}
        }

        std::thread::sleep(poll_interval);
    }

    assert!(ready, "Provider should become ready with empty directory");
    let entries: Vec<_> = fs::read_dir(&mount_path)
        .expect("Failed to read mount dir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(entries.is_empty(), "Mount path should be empty");

    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "linux")]
#[test]
fn local_provider_reads_file_content() {
    let mut fixture = TestFixture::new();
    assert!(fixture.wait_for_ready(), "Provider should be ready");

    std::thread::sleep(Duration::from_millis(500));

    assert!(
        fixture.wait_for_file("hello.txt", Duration::from_secs(3)),
        "File hello.txt should exist in mount"
    );

    let content =
        fs::read_to_string(fixture.mount_path.join("hello.txt")).expect("Failed to read hello.txt");
    assert_eq!(content, "Hello, World!");
}

#[cfg(feature = "fuse")]
mod macos_tests {
    use super::*;

    #[test]
    #[ignore = "requires macFUSE installed"]
    fn mount_local_directory_on_macos() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mount_path = temp_dir.path().join("mount");
        std::fs::create_dir(&mount_path).unwrap();

        let driver = crate::drivers::FuseDriver::new(
            mount_path.clone(),
            todo!("FUSE session would be created here in real usage"),
        );

        assert_eq!(driver.kind(), "macos");
        assert_eq!(driver.path(), &mount_path);
    }
}
