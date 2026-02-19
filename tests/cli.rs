//! Integration tests for the picman CLI.
//!
//! These tests run the actual binary against temporary libraries
//! to verify end-to-end behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Creates a minimal valid JPEG file (smallest valid JPEG possible).
fn create_test_image(path: &std::path::Path) {
    // Minimal valid JPEG: SOI + APP0 + DQT + SOF0 + DHT + SOS + EOI
    // This is a 1x1 red pixel JPEG
    let jpeg_bytes: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05,
        0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
        0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
        0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A,
        0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56,
        0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75,
        0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93,
        0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9,
        0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
        0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
        0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
        0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFB, 0xD5,
        0xDB, 0x20, 0xA8, 0xF1, 0x4A, 0x28, 0xA0, 0x02, 0x80, 0x0A, 0x28, 0x03, 0xFF, 0xD9,
    ];
    fs::write(path, jpeg_bytes).expect("Failed to write test image");
}

/// Helper to get picman command
fn picman() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("picman").unwrap()
}

/// Creates a test library with some images
fn setup_library() -> TempDir {
    let dir = TempDir::new().unwrap();

    // Create subdirectories
    fs::create_dir(dir.path().join("vacation")).unwrap();
    fs::create_dir(dir.path().join("vacation/beach")).unwrap();
    fs::create_dir(dir.path().join("family")).unwrap();

    // Create test images
    create_test_image(&dir.path().join("vacation/photo1.jpg"));
    create_test_image(&dir.path().join("vacation/photo2.jpg"));
    create_test_image(&dir.path().join("vacation/beach/sunset.jpg"));
    create_test_image(&dir.path().join("family/portrait.jpg"));

    dir
}

#[test]
fn init_creates_database() {
    let library = setup_library();

    picman()
        .arg("init")
        .arg(library.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"))
        .stdout(predicate::str::contains("directories"))
        .stdout(predicate::str::contains("files"));

    // Verify database was created
    assert!(library.path().join(".picman.db").exists());
}

#[test]
fn init_scans_all_directories() {
    let library = setup_library();

    picman()
        .arg("init")
        .arg(library.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("3 directories")); // vacation, vacation/beach, family
}

#[test]
fn init_scans_all_files() {
    let library = setup_library();

    picman()
        .arg("init")
        .arg(library.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("4 files")); // 4 images
}

#[test]
fn init_fails_on_nonexistent_path() {
    picman()
        .arg("init")
        .arg("/nonexistent/path/that/does/not/exist")
        .assert()
        .failure();
}

#[test]
fn sync_detects_new_files() {
    let library = setup_library();

    // Initialize first
    picman().arg("init").arg(library.path()).assert().success();

    // Add a new file
    create_test_image(&library.path().join("vacation/photo3.jpg"));

    // Sync should detect the new file (--full because mtime may not change within same second)
    picman()
        .arg("sync")
        .arg(library.path())
        .arg("--full")
        .assert()
        .success()
        .stdout(predicate::str::contains("+1").and(predicate::str::contains("files")));
}

#[test]
fn sync_detects_deleted_files() {
    let library = setup_library();

    // Initialize first
    picman().arg("init").arg(library.path()).assert().success();

    // Delete a file
    fs::remove_file(library.path().join("vacation/photo1.jpg")).unwrap();

    // Sync should detect the deletion (--full because mtime may not change within same second)
    picman()
        .arg("sync")
        .arg(library.path())
        .arg("--full")
        .assert()
        .success()
        .stdout(predicate::str::contains("-1").and(predicate::str::contains("files")));
}

#[test]
fn sync_requires_init_first() {
    let library = setup_library();

    picman()
        .arg("sync")
        .arg(library.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("No database found"));
}

#[test]
fn list_shows_all_files() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    picman()
        .arg("list")
        .arg(library.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("4 files"));
}

#[test]
fn rate_sets_rating() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    picman()
        .args(["rate", library.path().to_str().unwrap(), "vacation/photo1.jpg", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("5 stars"));
}

#[test]
fn rate_clears_rating_when_omitted() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    // Set a rating first
    picman()
        .args(["rate", library.path().to_str().unwrap(), "vacation/photo1.jpg", "5"])
        .assert()
        .success();

    // Clear it
    picman()
        .args(["rate", library.path().to_str().unwrap(), "vacation/photo1.jpg"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared rating"));
}

#[test]
fn list_filters_by_rating() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    // Rate one file
    picman()
        .args(["rate", library.path().to_str().unwrap(), "vacation/photo1.jpg", "5"])
        .assert()
        .success();

    // List with rating filter
    picman()
        .args(["list", library.path().to_str().unwrap(), "--rating", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 files"))
        .stdout(predicate::str::contains("photo1.jpg"));
}

#[test]
fn tag_adds_tag() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    picman()
        .args([
            "tag",
            library.path().to_str().unwrap(),
            "vacation/photo1.jpg",
            "--add",
            "favorite",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("favorite"));
}

#[test]
fn tag_removes_tag() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    // Add a tag first
    picman()
        .args([
            "tag",
            library.path().to_str().unwrap(),
            "vacation/photo1.jpg",
            "--add",
            "favorite",
        ])
        .assert()
        .success();

    // Remove it
    picman()
        .args([
            "tag",
            library.path().to_str().unwrap(),
            "vacation/photo1.jpg",
            "--remove",
            "favorite",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("no tags"));
}

#[test]
fn tag_lists_tags() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    // Add tags
    picman()
        .args([
            "tag",
            library.path().to_str().unwrap(),
            "vacation/photo1.jpg",
            "--add",
            "favorite",
            "--add",
            "sunset",
        ])
        .assert()
        .success();

    // List with --list flag
    picman()
        .args([
            "tag",
            library.path().to_str().unwrap(),
            "vacation/photo1.jpg",
            "--list",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("favorite"))
        .stdout(predicate::str::contains("sunset"));
}

#[test]
fn list_filters_by_tag() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    // Tag one file
    picman()
        .args([
            "tag",
            library.path().to_str().unwrap(),
            "vacation/photo1.jpg",
            "--add",
            "special",
        ])
        .assert()
        .success();

    // List with tag filter
    picman()
        .args(["list", library.path().to_str().unwrap(), "--tag", "special"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 files"))
        .stdout(predicate::str::contains("photo1.jpg"));
}

#[test]
fn repair_runs_on_healthy_library() {
    let library = setup_library();

    picman().arg("init").arg(library.path()).assert().success();

    picman()
        .args(["repair", library.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("All directory parent relationships are correct"));
}

#[test]
fn help_shows_usage() {
    picman()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Photo library management tool"))
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("sync"));
}

#[test]
fn nonexistent_library_path_fails() {
    // When no subcommand is given, the argument is treated as a library path
    picman()
        .arg("/nonexistent/library/path")
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

// =============================================================================
// Edge case tests: Directory moves and metadata preservation
// =============================================================================

#[test]
fn sync_moved_directory_preserves_file_ratings() {
    let library = setup_library();
    let lib_path = library.path().to_str().unwrap();

    picman().arg("init").arg(lib_path).assert().success();

    // Rate a file inside vacation/
    picman()
        .args(["rate", lib_path, "vacation/photo1.jpg", "5"])
        .assert()
        .success();

    // Move the directory on disk: vacation/ -> archive/vacation/ (same basename)
    // Move detection works by matching basenames
    fs::create_dir(library.path().join("archive")).unwrap();
    fs::rename(
        library.path().join("vacation"),
        library.path().join("archive/vacation"),
    )
    .unwrap();

    // Sync to detect the move (--full to avoid mtime-resolution false negatives)
    picman().arg("sync").arg(lib_path).arg("--full").assert().success();

    // The file should still have its rating at the new path
    picman()
        .args(["list", lib_path, "--rating", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("photo1.jpg"));
}

#[test]
fn sync_moved_directory_preserves_file_tags() {
    let library = setup_library();
    let lib_path = library.path().to_str().unwrap();

    picman().arg("init").arg(lib_path).assert().success();

    // Tag a file inside vacation/
    picman()
        .args(["tag", lib_path, "vacation/photo1.jpg", "--add", "favorite"])
        .assert()
        .success();

    // Move the directory on disk: vacation/ -> archive/vacation/ (same basename)
    // Move detection works by matching basenames
    fs::create_dir(library.path().join("archive")).unwrap();
    fs::rename(
        library.path().join("vacation"),
        library.path().join("archive/vacation"),
    )
    .unwrap();

    // Sync to detect the move (--full to avoid mtime-resolution false negatives)
    picman().arg("sync").arg(lib_path).arg("--full").assert().success();

    // The file should still have its tag at the new path
    picman()
        .args(["list", lib_path, "--tag", "favorite"])
        .assert()
        .success()
        .stdout(predicate::str::contains("photo1.jpg"));
}

#[test]
fn sync_moved_nested_directory_preserves_file_metadata() {
    let library = setup_library();
    let lib_path = library.path().to_str().unwrap();

    picman().arg("init").arg(lib_path).assert().success();

    // Rate and tag a file in nested directory vacation/beach/
    picman()
        .args(["rate", lib_path, "vacation/beach/sunset.jpg", "5"])
        .assert()
        .success();
    picman()
        .args(["tag", lib_path, "vacation/beach/sunset.jpg", "--add", "sunset"])
        .assert()
        .success();

    // Move parent directory: vacation/ -> trips/
    fs::rename(library.path().join("vacation"), library.path().join("trips")).unwrap();

    // Sync (--full to avoid mtime-resolution false negatives)
    picman().arg("sync").arg(lib_path).arg("--full").assert().success();

    // File should still have rating and tag
    picman()
        .args(["list", lib_path, "--rating", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sunset.jpg"));

    picman()
        .args(["list", lib_path, "--tag", "sunset"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sunset.jpg"));
}

#[test]
fn sync_moved_directory_with_modified_file_loses_metadata() {
    // If a file inside is modified (different mtime), it's treated as a new file
    let library = setup_library();
    let lib_path = library.path().to_str().unwrap();

    picman().arg("init").arg(lib_path).assert().success();

    // Rate a file
    picman()
        .args(["rate", lib_path, "vacation/photo1.jpg", "5"])
        .assert()
        .success();

    // Modify the file content (changes mtime)
    std::thread::sleep(std::time::Duration::from_millis(100));
    fs::write(library.path().join("vacation/photo1.jpg"), "modified content").unwrap();

    // Move the directory
    fs::rename(library.path().join("vacation"), library.path().join("holiday")).unwrap();

    // Sync (--full to avoid mtime-resolution false negatives)
    picman().arg("sync").arg(lib_path).arg("--full").assert().success();

    // File should NOT have rating anymore (it's a "new" file due to modification)
    picman()
        .args(["list", lib_path, "--rating", "5"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 files"));
}
