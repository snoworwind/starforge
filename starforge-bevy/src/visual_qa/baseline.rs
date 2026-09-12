//! A01 baseline metadata for visual QA runs.
//!
//! The collector only *reads* the repository, the window and the render
//! adapter. It never writes to `saves/`, never re-saves `settings.json`, and
//! reports missing information as `None` instead of guessing. Two runs from
//! the same commit therefore produce comparable baseline blocks.

use std::path::{Path, PathBuf};

use bevy::diagnostic::SystemInfo;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::Window;
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct SourceStats {
    pub files: usize,
    pub physical_lines: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AssetGroup {
    pub extension: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AssetsSnapshot {
    pub files: usize,
    pub bytes: u64,
    /// FNV-1a over sorted `relative/path|bytes` entries; stable across
    /// machines as long as the file set and sizes are unchanged.
    pub manifest_hash: Option<String>,
    pub groups: Vec<AssetGroup>,
    /// Directories that are required for the full-quality path but are
    /// gitignored (large local payloads). Recorded as present/missing so a
    /// "minimal assets" run cannot be mistaken for the full-material result.
    pub ignored_payloads: Vec<String>,
    /// Ignore files that decide which asset payloads are tracked.
    pub ignore_files: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HardwareSnapshot {
    pub os: String,
    pub arch: String,
    pub cpu_threads: usize,
    pub cpu_model: Option<String>,
    pub system_memory: Option<String>,
    pub gpu_name: Option<String>,
    pub gpu_backend: Option<String>,
    pub gpu_device_type: Option<String>,
    pub gpu_driver: Option<String>,
    pub gpu_driver_info: Option<String>,
    pub window_width: u32,
    pub window_height: u32,
    pub window_scale: f32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Baseline {
    pub commit: Option<String>,
    pub crate_root: Option<String>,
    pub src: SourceStats,
    pub assets: AssetsSnapshot,
}

/// Locate the crate root without depending on the process working directory.
/// `CARGO_MANIFEST_DIR` covers `cargo run`/`cargo test`; the ancestor walk
/// covers release executables started from `target/<profile>/`.
pub fn find_crate_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(dir);
        if candidate.join("Cargo.toml").is_file() && candidate.join("src").is_dir() {
            return Some(candidate);
        }
    }
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        starts.push(parent.to_path_buf());
    }
    for start in starts {
        let mut dir = Some(start.as_path());
        let mut depth = 0;
        while let Some(current) = dir {
            if current.join("Cargo.toml").is_file() && current.join("src").is_dir() {
                return Some(current.to_path_buf());
            }
            if depth >= 6 {
                break;
            }
            dir = current.parent();
            depth += 1;
        }
    }
    None
}

/// Read the repository `HEAD` without spawning git. The crate lives inside the
/// repository, so `.git` is searched from `root` upward. Handles both a normal
/// `.git` directory and the `gitdir:` pointer file used by worktrees.
pub fn git_head(root: &Path) -> Option<String> {
    let dot_git = find_dot_git(root)?;
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim().to_string();
        let target = PathBuf::from(target);
        if target.is_absolute() {
            target
        } else {
            dot_git.parent().unwrap_or(root).join(target)
        }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let hash = if let Some(reference) = head.strip_prefix("ref:") {
        std::fs::read_to_string(git_dir.join(reference.trim())).ok()?
    } else {
        head.to_string()
    };
    let short: String = hash.trim().chars().take(12).collect();
    if short.is_empty() { None } else { Some(short) }
}

fn find_dot_git(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    let mut depth = 0;
    while let Some(current) = dir {
        let candidate = current.join(".git");
        if candidate.is_dir() || candidate.is_file() {
            return Some(candidate);
        }
        if depth >= 6 {
            break;
        }
        dir = current.parent();
        depth += 1;
    }
    None
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn visit_files(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => visit_files(&path, out),
            Ok(kind) if kind.is_file() => {
                let bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                out.push((path, bytes));
            }
            _ => {}
        }
    }
}

fn collect_source(root: &Path) -> SourceStats {
    let mut files = Vec::new();
    visit_files(&root.join("src"), &mut files);
    let mut stats = SourceStats::default();
    for (path, _) in files {
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        stats.files += 1;
        if let Ok(text) = std::fs::read_to_string(&path) {
            stats.physical_lines += text.lines().count();
        }
    }
    stats
}

fn collect_assets(root: &Path) -> AssetsSnapshot {
    let assets_dir = root.join("assets");
    let mut files = Vec::new();
    visit_files(&assets_dir, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut snapshot = AssetsSnapshot::default();
    let mut groups: std::collections::BTreeMap<String, (usize, u64)> =
        std::collections::BTreeMap::new();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for (path, bytes) in &files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("(none)")
            .to_ascii_lowercase();
        let entry = groups.entry(extension).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += bytes;
        snapshot.files += 1;
        snapshot.bytes += bytes;
        hash = fnv1a64(format!("{relative}|{bytes}\n").as_bytes())
            .wrapping_mul(hash ^ 0x9e37_79b9_7f4a_7c15);
    }
    snapshot.manifest_hash = Some(format!("{hash:016x}"));
    snapshot.groups = groups
        .into_iter()
        .map(|(extension, (files, bytes))| AssetGroup {
            extension,
            files,
            bytes,
        })
        .collect();

    let payloads = [
        ("assets/models/external", "external ships/stations"),
        ("assets/models/earth", "origin star Earth model"),
    ];
    for (relative, label) in payloads {
        let present = root.join(relative).is_dir();
        let state = if present { "present" } else { "missing" };
        snapshot
            .ignored_payloads
            .push(format!("{relative} ({label}, gitignored): {state}"));
    }
    if let Some(repo_root) = root.parent() {
        let present = repo_root.join("models").is_dir();
        let state = if present { "present" } else { "missing" };
        snapshot
            .ignored_payloads
            .push(format!("../models (local model pack, gitignored): {state}"));
    }
    for relative in [".gitignore", "../.gitignore"] {
        if root.join(relative).is_file() {
            snapshot.ignore_files.push(relative.to_string());
        }
    }
    snapshot
}

pub fn collect() -> Baseline {
    let root = find_crate_root();
    let mut baseline = Baseline {
        commit: root.as_deref().and_then(git_head),
        crate_root: root.as_ref().map(|path| path.display().to_string()),
        ..Default::default()
    };
    if let Some(root) = &root {
        baseline.src = collect_source(root);
        baseline.assets = collect_assets(root);
    }
    baseline
}

pub fn collect_hardware(
    adapter: Option<&RenderAdapterInfo>,
    window: Option<&Window>,
    system: Option<&SystemInfo>,
) -> HardwareSnapshot {
    let mut hardware = HardwareSnapshot {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_threads: std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(0),
        cpu_model: system.map(|info| info.cpu.clone()),
        system_memory: system.map(|info| info.memory.clone()),
        ..Default::default()
    };
    if let Some(info) = adapter {
        hardware.gpu_name = Some(info.name.clone());
        hardware.gpu_backend = Some(format!("{:?}", info.backend));
        hardware.gpu_device_type = Some(format!("{:?}", info.device_type));
        hardware.gpu_driver = Some(info.driver.clone());
        hardware.gpu_driver_info = Some(info.driver_info.clone());
    }
    if let Some(window) = window {
        hardware.window_width = window.resolution.physical_width();
        hardware.window_height = window.resolution.physical_height();
        hardware.window_scale = window.resolution.scale_factor();
    }
    hardware
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_reference_values() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(fnv1a64(b"abc"), fnv1a64(b"abd"));
    }

    #[test]
    fn git_head_reads_ref_and_detached_forms() {
        let dir = std::env::temp_dir().join(format!(
            "starforge-visual-qa-git-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            git.join("refs/heads/main"),
            "c6f0c1d1aaba4af47d9f589a3ae67e80\n",
        )
        .unwrap();
        assert_eq!(git_head(&dir).as_deref(), Some("c6f0c1d1aaba"));
        // The crate root can sit below the repository root like starforge-bevy.
        let crate_root = dir.join("crate").join("starforge-bevy");
        std::fs::create_dir_all(&crate_root).unwrap();
        assert_eq!(git_head(&crate_root).as_deref(), Some("c6f0c1d1aaba"));
        std::fs::write(git.join("HEAD"), "0123456789abcdef0123456789abcdef\n").unwrap();
        assert_eq!(git_head(&crate_root).as_deref(), Some("0123456789ab"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
