#![allow(clippy::unwrap_used)]

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
    data_path: PathBuf,
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
                "provide",
                "--path",
                mount_path.to_str().unwrap(),
                "local",
                "--root",
                data_path.to_str().unwrap(),
            ])
            .spawn()
            .expect("Failed to spawn provider");

        Self {
            mount_path,
            data_path,
            child,
        }
    }

    fn wait_for_ready(&self) -> bool {
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
    assert!(fixture.wait_for_ready());
}

#[cfg(target_os = "linux")]
#[test]
fn local_provider_lists_directory_contents() {
    let mut fixture = TestFixture::new();
    assert!(fixture.wait_for_ready());

    let entries: Vec<_> = fs::read_dir(&fixture.mount_path)
        .expect("Failed to read mount dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    assert!(entries.contains(&"hello.txt".to_string()));
    assert!(entries.contains(&"subdir".to_string()));
}

#[cfg(target_os = "linux")]
#[test]
fn local_provider_reads_file_content() {
    let mut fixture = TestFixture::new();
    assert!(fixture.wait_for_ready());

    let content =
        fs::read_to_string(fixture.mount_path.join("hello.txt")).expect("Failed to read hello.txt");
    assert_eq!(content, "Hello, World!");
}
