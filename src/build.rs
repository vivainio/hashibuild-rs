//! Build-and-cache step: look up `<name>_<hash>.zip` in the archive (local, then remote),
//! unzip it on a hit; on a miss run the build command, zip the outputs and run the uploader.
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let tmp = to.with_extension("part");
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
    let tmp = zip_path.with_extension("tmp");
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
