# LocalStorage Provider System Test - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Create an integration test that starts a LocalStorage provider inline, verifies it's connected, and validates directory operations.

**Architecture:** Test creates a temp directory, spawns `provide --path <mount> local --root <data>` (ad-hoc), polls mount accessibility + process aliveness (50ms interval, 5s timeout), verifies operations.

**Parameters (confirmed):**
- Poll interval: 50ms
- Timeout: 5 seconds
- Run in CI: Yes

---

## Task 1: Create Integration Test File

**Files:** Create `tests/system/local_provider_test.rs`

- [ ] Write `TestFixture` struct:
  - Creates temp dir with `data/` and `mnt/` subdirs
  - Spawns ad-hoc provider via `std::process::Command`
  - `wait_for_ready(timeout)` polls `fs::read_dir(mount)` (ready), `child.try_wait()` (crashed), timeout (failed)
  - `Drop` impl kills child
- [ ] `local_provider_connects_and_responds` - ready check
- [ ] `local_provider_lists_directory_contents` - verify mount shows files
- [ ] `local_provider_reads_file_content` - verify file reads

## Task 2: Add Edge Case Tests

**Files:** Modify `tests/system/local_provider_test.rs`

- [ ] `provider_fails_on_invalid_root` - bad path error
- [ ] `provider_cleans_up_on_drop` - process terminates
- [ ] `provider_timeout_on_unavailable_mount` - blocked mount scenario

## Task 3: Run and Debug

- [ ] `cargo test --test local_provider_test -- --nocapture`
- [ ] Debug platform differences if needed

## Task 4: Documentation

**Files:** Modify `README.md`

- [ ] Add testing section for system tests
