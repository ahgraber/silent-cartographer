//! Materialize and verify the embedding model the binary compiles in.
//!
//! The model payload (`models/potion-code-16M-v2/`) is not committed; the committed manifest
//! (`models/potion-code-16M-v2.manifest.json`) pins the upstream repository, revision, and
//! per-file SHA-256. This script downloads any missing file from the pinned revision and verifies
//! every file against the manifest on every build, so the bytes `include_bytes!` compiles in are
//! always exactly the manifest's — a drifted or corrupted payload fails the build, never ships.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(serde::Deserialize)]
struct Manifest {
    source: String,
    revision: String,
    files: BTreeMap<String, String>,
}

fn main() {
    let manifest_path = "models/potion-code-16M-v2.manifest.json";
    let payload_dir = Path::new("models/potion-code-16M-v2");
    println!("cargo:rerun-if-changed={manifest_path}");

    let manifest: Manifest = serde_json::from_str(
        &std::fs::read_to_string(manifest_path).unwrap_or_else(|e| panic!("reading {manifest_path}: {e}")),
    )
    .unwrap_or_else(|e| panic!("parsing {manifest_path}: {e}"));

    std::fs::create_dir_all(payload_dir).expect("creating the model payload directory");
    for (name, expected_sha256) in &manifest.files {
        let path = payload_dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        if !path.exists() {
            fetch(&manifest.source, &manifest.revision, name, &path);
        }
        let actual = sha256_of(&path);
        assert_eq!(
            &actual,
            expected_sha256,
            "{} does not match the manifest ({manifest_path}); delete the file and rebuild to refetch \
             the pinned revision {}",
            path.display(),
            manifest.revision
        );
    }
}

/// Download one model file from the pinned upstream revision via `curl`.
///
/// The download lands in a temporary sibling and is renamed into place, so an interrupted fetch
/// never leaves a partial file where the checksum pass would name it corrupt.
fn fetch(source: &str, revision: &str, name: &str, path: &Path) {
    let url = format!("{source}/resolve/{revision}/{name}");
    let temp = path.with_extension("fetch-tmp");
    let status = std::process::Command::new("curl")
        .args(["-sSfL", "--retry", "2", "-o"])
        .arg(&temp)
        .arg(&url)
        .status();
    let ok = status.as_ref().is_ok_and(|s| s.success());
    if !ok {
        let _ = std::fs::remove_file(&temp);
        panic!(
            "could not fetch {url} (is the network available?); download the files listed in \
             models/potion-code-16M-v2.manifest.json into {} by hand and rebuild",
            path.parent().map(|p| p.display().to_string()).unwrap_or_default()
        );
    }
    std::fs::rename(&temp, path).expect("moving the fetched model file into place");
}

/// The lowercase-hex SHA-256 of a file's contents.
fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let digest = Sha256::digest(&bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
