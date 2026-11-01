//! Build-and-cache step: look up `<name>_<hash>.zip` in the archive (local, then remote),
//! unzip it on a hit; on a miss run the build command, zip the outputs and run the uploader.
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

pub struct Plan {
    pub name: String,
    pub hash: String,
    pub input_root: PathBuf,
    pub output_root: PathBuf,
    pub output_dirs: Vec<String>,
    pub build_cmd: String,
    pub archive: PathBuf,
    pub fetcher: Option<String>,
    pub uploader: Option<String>,
}

/// Per-process temp name next to `to`, so concurrent runs never write the same temp file.
fn temp_path(to: &Path, ext: &str) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{ext}", std::process::id()));
    to.with_file_name(name)
}

fn shell(cmd: &str, dir: &Path) -> Result<(), String> {
    eprintln!("> {cmd}");
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    // Keep stdout clean for the hash: send child output to stderr.
    let st = c
        .current_dir(dir)
        .stdout(io::stderr())
        .status()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("command failed ({st}): {cmd}"))
    }
}

/// Run the fetch command to download the archive into `to`. Any failure (non-zero exit,
/// no file produced) is a cache miss. Written to a temp name first, then renamed.
fn fetch(cmd: &str, zip_name: &str, to: &Path, dir: &Path) -> bool {
    let tmp = temp_path(to, "part");
    let _ = fs::remove_file(&tmp);
    let abs = if tmp.is_absolute() {
        tmp.clone()
    } else {
        std::env::current_dir().unwrap_or_default().join(&tmp)
    };
    let cmd = cmd
        .replace("[ZIP]", zip_name)
        .replace("[DEST]", &abs.to_string_lossy());
    if shell(&cmd, dir).is_err() || !tmp.is_file() || fs::rename(&tmp, to).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    true
}

fn add_dir(
    zw: &mut zip::ZipWriter<File>,
    root: &Path,
    dir: &Path,
    opts: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let e = |e: &dyn std::fmt::Display| format!("{}: {e}", dir.display());
    let mut ents: Vec<_> = fs::read_dir(dir)
        .map_err(|x| e(&x))?
        .collect::<Result<_, _>>()
        .map_err(|x| e(&x))?;
    ents.sort_by_key(|d| d.file_name());
    for d in ents {
        let p = d.path();
        let name = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let md = fs::metadata(&p).map_err(|x| e(&x))?;
        if md.is_dir() {
            zw.add_directory(format!("{name}/"), opts)
                .map_err(|x| e(&x))?;
            add_dir(zw, root, &p, opts)?;
        } else {
            #[cfg(unix)]
            let opts = {
                use std::os::unix::fs::PermissionsExt;
                opts.unix_permissions(md.permissions().mode())
            };
            zw.start_file(name, opts).map_err(|x| e(&x))?;
            io::copy(&mut File::open(&p).map_err(|x| e(&x))?, zw).map_err(|x| e(&x))?;
        }
    }
    Ok(())
}

fn zip_outputs(plan: &Plan, zip_path: &Path) -> Result<(), String> {
    let tmp = temp_path(zip_path, "tmp");
    let f = File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for d in &plan.output_dirs {
        let p = plan.output_root.join(d);
        if !p.is_dir() {
            let _ = fs::remove_file(&tmp);
            return Err(format!("output dir missing after build: {}", p.display()));
        }
        let name = d.trim_end_matches('/').replace('\\', "/");
        zw.add_directory(format!("{name}/"), opts)
            .map_err(|e| e.to_string())?;
        add_dir(&mut zw, &plan.output_root, &p, opts)?;
    }
    zw.finish().map_err(|e| e.to_string())?;
    // Atomic publish so concurrent builds never see a half-written archive.
    fs::rename(&tmp, zip_path).map_err(|e| e.to_string())
}

fn unzip(zip_path: &Path, to: &Path) -> Result<(), String> {
    let f = File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for i in 0..z.len() {
        let mut ent = z.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name rejects absolute paths and `..` (zip-slip).
        let Some(rel) = ent.enclosed_name() else {
            return Err(format!("unsafe path in archive: {:?}", ent.name()));
        };
        let out = to.join(rel);
        if ent.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mode = ent.unix_mode();
        io::copy(
            &mut ent,
            &mut File::create(&out).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(m) = mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(m));
        }
        let _ = mode;
    }
    Ok(())
}

/// Returns Ok(true) if the build was skipped thanks to a cached archive.
pub fn run(plan: &Plan) -> Result<bool, String> {
    fs::create_dir_all(&plan.archive).map_err(|e| format!("{}: {e}", plan.archive.display()))?;
    let zip_name = format!("{}_{}.zip", plan.name, plan.hash);
    let zip_path = plan.archive.join(&zip_name);

    let mut found = zip_path.is_file();
    if !found && let Some(cmd) = &plan.fetcher {
        found = fetch(cmd, &zip_name, &zip_path, &plan.input_root);
    }
    if found {
        eprintln!(
            "cache hit: unzip {} into {}",
            zip_path.display(),
            plan.output_root.display()
        );
        unzip(&zip_path, &plan.output_root)?;
        return Ok(true);
    }

    eprintln!("cache miss: building ({zip_name})");
    shell(&plan.build_cmd, &plan.input_root)?;
    zip_outputs(plan, &zip_path)?;
    if let Some(up) = &plan.uploader {
        let abs = zip_path.canonicalize().unwrap_or(zip_path);
        let cmd = up.replace("[ZIP]", &abs.to_string_lossy());
        // Like hashibuild: an upload failure must not fail the build.
        if let Err(e) = shell(&cmd, &plan.input_root) {
            eprintln!("warning: uploader failed: {e}");
        }
    }
    Ok(false)
}

/// Delete archive zips older than `max_age`, then keep only the newest ones that fit within
/// `max_bytes`. Only `*.zip` files directly in `dir` are touched. Returns (files removed, bytes freed).
pub fn vacuum(dir: &Path, max_age: Duration, max_bytes: u64) -> Result<(usize, u64), String> {
    if !dir.is_dir() {
        return Ok((0, 0));
    }
    let now = SystemTime::now();
    let mut zips = Vec::new();
    for ent in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let ent = ent.map_err(|e| e.to_string())?;
        let p = ent.path();
        let ext = p
            .extension()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(md) = ent.metadata()
            && md.is_file()
            && matches!(ext.as_str(), "zip" | "tmp" | "part")
        {
            // Leftover temp files only count once they are old: a fresh one may be in use.
            let mtime = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if ext != "zip" && now.duration_since(mtime).is_ok_and(|d| d <= max_age) {
                continue;
            }
            zips.push((p, md.len(), md.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
    zips.sort_by_key(|z| std::cmp::Reverse(z.2)); // newest first
    let (mut kept, mut removed, mut freed) = (0u64, 0usize, 0u64);
    for (p, len, mtime) in zips {
        let too_old = now.duration_since(mtime).is_ok_and(|d| d > max_age);
        if !too_old {
            kept += len;
        }
        if too_old || kept > max_bytes {
            fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            removed += 1;
            freed += len;
        }
    }
    Ok((removed, freed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with_age(dir: &Path, name: &str, bytes: usize, age_days: u64) {
        let p = dir.join(name);
        fs::write(&p, vec![0u8; bytes]).unwrap();
        let t = SystemTime::now() - Duration::from_secs(age_days * 86400);
        File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn vacuum_removes_old_zips_only() {
        let d = tempfile::tempdir().unwrap();
        zip_with_age(d.path(), "new.zip", 10, 0);
        zip_with_age(d.path(), "old.zip", 10, 10);
        fs::write(d.path().join("notes.txt"), "keep").unwrap();
        let (n, freed) = vacuum(d.path(), Duration::from_secs(3 * 86400), 1 << 20).unwrap();
        assert_eq!((n, freed), (1, 10));
        assert!(d.path().join("new.zip").exists());
        assert!(!d.path().join("old.zip").exists());
        assert!(d.path().join("notes.txt").exists());
    }

    #[test]
    fn vacuum_keeps_newest_within_size_cap() {
        let d = tempfile::tempdir().unwrap();
        zip_with_age(d.path(), "a.zip", 100, 0);
        zip_with_age(d.path(), "b.zip", 100, 1);
        zip_with_age(d.path(), "c.zip", 100, 2);
        let (n, _) = vacuum(d.path(), Duration::from_secs(30 * 86400), 250).unwrap();
        assert_eq!(n, 1);
        assert!(d.path().join("a.zip").exists() && d.path().join("b.zip").exists());
        assert!(!d.path().join("c.zip").exists());
    }

    #[test]
    fn vacuum_missing_dir_is_noop() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            vacuum(&d.path().join("nope"), Duration::ZERO, 0).unwrap(),
            (0, 0)
        );
    }

    #[test]
    fn vacuum_removes_only_stale_temp_files() {
        let d = tempfile::tempdir().unwrap();
        zip_with_age(d.path(), "x.zip.123.tmp", 5, 10);
        zip_with_age(d.path(), "y.zip.456.part", 5, 10);
        zip_with_age(d.path(), "z.zip.789.tmp", 5, 0);
        let (n, _) = vacuum(d.path(), Duration::from_secs(3 * 86400), 1 << 20).unwrap();
        assert_eq!(n, 2);
        assert!(d.path().join("z.zip.789.tmp").exists());
    }
}
