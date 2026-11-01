mod build;
use clap::Parser;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Names of files that pin dependency / toolchain versions. They are picked up
/// from the subtree's ancestor directories (up to the repo root), since the
/// subtree's build result depends on them even though they live outside it.
const VERSION_FILES: &[&str] = &[
    // Lock files and toolchain pins only: manifests in ancestor dirs change for
    // unrelated reasons and would cause spurious hash changes. Add them with -n.
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
    ".nvmrc",
    ".node-version",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "pdm.lock",
    ".python-version",
    "go.sum",
    "go.work",
    "go.work.sum",
    "Gemfile.lock",
    ".ruby-version",
    "composer.lock",
    "gradle.lockfile",
    "packages.lock.json",
    "global.json",
    "mix.lock",
    "pubspec.lock",
    "Package.resolved",
    "Podfile.lock",
    "flake.lock",
    ".tool-versions",
];

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Config {
    input_root: Option<String>,
    include: Vec<String>,
    exclude: Vec<String>,
    salt: Option<String>,
    build_param: Option<String>,
    name: Option<String>,
    output_root: Option<String>,
    output_dir: Option<String>,
    output_dirs: Vec<String>,
    build_cmd: Option<String>,
}

fn archive_dir(a: &Args) -> Result<PathBuf, String> {
    a.archive
        .clone()
        .or_else(|| std::env::var_os("HASHIBUILD_ARCHIVE").map(PathBuf::from))
        .ok_or_else(|| "need --archive or HASHIBUILD_ARCHIVE".to_string())
}

fn excluded(rel: &str, only: &[String], exclude: &[String]) -> bool {
    let l = rel.to_lowercase();
    let pre = |v: &String| l.starts_with(&v.to_lowercase());
    if exclude.iter().any(pre) {
        return true;
    }
    !only.is_empty() && !only.iter().any(pre)
}

#[derive(Parser)]
#[command(
    about = "Hash a source subtree: its git-tracked files plus version-pinning files (lock files etc.) from parent dirs"
)]
struct Args {
    /// Subtree directory (default: current directory, or inputRoot from --config)
    path: Option<PathBuf>,
    /// hashibuild-style JSON config (inputRoot, include, exclude, salt, buildParam; other keys ignored)
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Keep only subtree files whose subtree-relative path starts with one of these
    /// prefixes (case-insensitive, repeatable). Applied before --exclude.
    #[arg(long)]
    only: Vec<String>,
    /// Drop subtree files whose relative path starts with one of these prefixes (case-insensitive)
    #[arg(long)]
    exclude: Vec<String>,
    /// Salt mixed into the hash, to invalidate otherwise identical hashes
    #[arg(long)]
    salt: Option<String>,
    /// Build parameter mixed into the hash
    #[arg(long)]
    build_param: Option<String>,
    /// Build with caching: unzip <archive>/<name>_<hash>.zip if present (fetching it from
    /// HASHIBUILD_FETCHER command if needed), else run buildCmd, zip outputs, run
    /// HASHIBUILD_UPLOADER. Needs --config (or --build-cmd/--output-dir).
    #[arg(short, long)]
    build: bool,
    /// Local archive dir (default: $HASHIBUILD_ARCHIVE)
    #[arg(long)]
    archive: Option<PathBuf>,
    /// Build command (overrides config buildCmd); [BUILDPARAM] is replaced by --build-param
    #[arg(long)]
    build_cmd: Option<String>,
    /// Output dir relative to output root (repeatable; adds to config outputDir(s))
    #[arg(long)]
    output_dir: Vec<String>,
    /// Delete old/excess archive zips from the archive dir (alone, or before --build)
    #[arg(long)]
    vacuum: bool,
    /// --vacuum: remove archives older than this many days
    #[arg(long, default_value_t = 3)]
    max_age_days: u64,
    /// --vacuum: keep at most this many MB of the newest archives
    #[arg(long, default_value_t = 500)]
    max_size_mb: u64,
    /// Print per-file checksums ("path sha256") and the total
    #[arg(short, long)]
    manifest: bool,
    /// Extra repo-relative files/dirs/pathspecs to include (repeatable)
    #[arg(short, long)]
    include: Vec<String>,
    /// Extra file names to treat as version files in ancestor dirs (repeatable)
    #[arg(short = 'n', long = "version-file")]
    version_file: Vec<String>,
    /// Don't include the built-in version files list
    #[arg(long)]
    no_defaults: bool,
    /// Also include untracked, non-ignored files
    #[arg(long)]
    untracked: bool,
    /// Print the files that go into the hash instead of just the digest
    #[arg(short, long)]
    list: bool,
    /// Print only the first N hex chars
    #[arg(short, long)]
    short: Option<usize>,
}

fn git(dir: &Path, args: &[&str], pathspecs: &[String]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    if !pathspecs.is_empty() {
        cmd.arg("--").args(pathspecs);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

fn split_nul(b: &[u8]) -> Vec<String> {
    b.split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

fn run() -> Result<(), String> {
    let mut a = Args::parse();
    if a.vacuum {
        let dir = archive_dir(&a)?;
        let (n, bytes) = build::vacuum(
            &dir,
            std::time::Duration::from_secs(a.max_age_days * 86400),
            a.max_size_mb * 1024 * 1024,
        )?;
        eprintln!(
            "vacuum: removed {n} archive(s), {} KB from {}",
            bytes / 1024,
            dir.display()
        );
        if !a.build && a.path.is_none() && a.config.is_none() {
            return Ok(());
        }
    }
    let mut cfg = Config::default();
    let mut base = PathBuf::from(".");
    if let Some(cp) = &a.config {
        let txt = std::fs::read_to_string(cp).map_err(|e| format!("{}: {e}", cp.display()))?;
        cfg = serde_json::from_str(&txt).map_err(|e| format!("{}: {e}", cp.display()))?;
        base = cp.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        if base.as_os_str().is_empty() {
            base = PathBuf::from(".");
        }
    }
    let path = match (&a.path, &cfg.input_root) {
        (Some(p), _) => p.clone(),
        (None, Some(r)) => base.join(r),
        (None, None) => PathBuf::from("."),
    };
    a.only.extend(cfg.include);
    a.exclude.extend(cfg.exclude);
    let name = cfg.name.take();
    let build_cmd = a.build_cmd.take().or(cfg.build_cmd.take());
    let out_root_cfg = cfg.output_root.take();
    let mut out_dirs = cfg.output_dirs.clone();
    out_dirs.extend(cfg.output_dir.take());
    out_dirs.extend(a.output_dir.iter().cloned());
    let salt = a.salt.take().or(cfg.salt);
    let build_param = a.build_param.take().or(cfg.build_param);
    let sub = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let top = String::from_utf8(git(&sub, &["rev-parse", "--show-toplevel"], &[])?)
        .map_err(|e| e.to_string())?;
    let root = PathBuf::from(top.trim())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let rel = sub
        .strip_prefix(&root)
        .map_err(|_| "path outside repo".to_string())?
        .to_path_buf();

    // Key = label hashed; value = file on disk. BTreeMap gives deterministic order.
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();

    // 1. Subtree files, labelled relative to the subtree (so moving it keeps the hash).
    let mut ls = vec!["ls-files", "-z", "--cached"];
    if a.untracked {
        ls.extend(["--others", "--exclude-standard"]);
    }
    for f in split_nul(&git(&sub, &ls, &[".".to_string()])?) {
        if excluded(&f, &a.only, &a.exclude) {
            continue;
        }
        files.insert(format!("./{f}"), sub.join(&f));
    }

    // 2. Version files in each ancestor dir (subtree itself is already covered).
    let mut names: Vec<&str> = if a.no_defaults {
        vec![]
    } else {
        VERSION_FILES.to_vec()
    };
    names.extend(a.version_file.iter().map(|s| s.as_str()));
    let mut specs: Vec<String> = Vec::new();
    let mut dir = rel.clone();
    while dir.pop() || dir.as_os_str().is_empty() {
        for n in &names {
            specs.push(format!(":(top,literal){}", dir.join(n).display()));
        }
        if dir.as_os_str().is_empty() {
            break;
        }
    }
    specs.extend(a.include.iter().map(|s| format!(":(top){s}")));
    if !specs.is_empty() {
        for f in split_nul(&git(&root, &["ls-files", "-z", "--cached"], &specs)?) {
            let full = root.join(&f);
            if !full.starts_with(&sub) {
                files.insert(format!("/{f}"), full);
            }
        }
    }

    let mut h = Sha256::new();
    h.update(b"hashibuild:v1\0");
    for (label, p) in &files {
        // Tracked-but-deleted files are skipped.
        let mut exec = false;
        let content = match std::fs::symlink_metadata(p) {
            Ok(m) if m.file_type().is_symlink() => std::fs::read_link(p)
                .map(|t| t.to_string_lossy().into_owned().into_bytes())
                .map_err(|e| format!("{}: {e}", p.display()))?,
            Ok(m) if m.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    exec = m.permissions().mode() & 0o111 != 0;
                }
                std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?
            }
            _ => continue,
        };
        let fd = hex::encode(Sha256::digest(&content));
        if a.manifest {
            println!("{label} {fd}{}", if exec { " x" } else { "" });
        } else if a.list {
            println!("{label}");
        }
        h.update((label.len() as u64).to_le_bytes());
        h.update(label.as_bytes());
        h.update([exec as u8]);
        h.update(fd.as_bytes());
    }
    for (tag, v) in [("salt", &salt), ("param", &build_param)] {
        if let Some(v) = v {
            h.update(format!("\0{tag}:{}:", v.len()).as_bytes());
            h.update(v.as_bytes());
        }
    }
    let mut digest = hex::encode(h.finalize());
    if let Some(n) = a.short {
        digest.truncate(n);
    }
    if a.manifest {
        println!("Total: {digest}");
    } else {
        println!("{digest}");
    }

    if a.build {
        let archive = archive_dir(&a)?;
        let mut cmd = build_cmd.ok_or("--build needs buildCmd in config or --build-cmd")?;
        if let Some(bp) = &build_param {
            cmd = cmd.replace("[BUILDPARAM]", bp);
        }
        if out_dirs.is_empty() {
            return Err("--build needs outputDir/outputDirs in config or --output-dir".into());
        }
        let output_root = match out_root_cfg {
            Some(r) => base
                .join(r)
                .canonicalize()
                .map_err(|e| format!("outputRoot: {e}"))?,
            None => sub.clone(),
        };
        let name = name.unwrap_or_else(|| {
            sub.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "build".into())
        });
        let skipped = build::run(&build::Plan {
            name,
            hash: digest.clone(),
            input_root: sub.clone(),
            output_root,
            output_dirs: out_dirs,
            build_cmd: cmd,
            archive,
            fetcher: std::env::var("HASHIBUILD_FETCHER")
                .ok()
                .filter(|s| !s.is_empty()),
            uploader: std::env::var("HASHIBUILD_UPLOADER")
                .ok()
                .filter(|s| !s.is_empty()),
        })?;
        eprintln!(
            "{}",
            if skipped {
                "restored from archive"
            } else {
                "built and archived"
            }
        );
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::excluded;

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn filters_are_case_insensitive_prefixes() {
        assert!(!excluded("src/a.rs", &[], &[]));
        assert!(excluded("Src/Tests/a.rs", &[], &v(&["src/tests"])));
        assert!(!excluded("src/a.rs", &v(&["SRC"]), &[]));
        assert!(excluded("docs/a.md", &v(&["src"]), &[]));
    }

    #[test]
    fn exclude_wins_over_only() {
        assert!(excluded("src/tests/a.rs", &v(&["src"]), &v(&["src/tests"])));
    }
}
