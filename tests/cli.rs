//! End-to-end tests: run the real binary against throwaway git repos.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hashibuild"))
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

/// repo/ { Cargo.lock, other.txt, svc/{Cargo.toml, a/f.rs, tests/t.rs} }, all tracked.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().canonicalize().unwrap();
    git(&r, &["init", "-q"]);
    write(&r, "Cargo.lock", "lock1");
    write(&r, "other.txt", "unrelated");
    write(&r, "svc/Cargo.toml", "toml");
    write(&r, "svc/a/f.rs", "fn main() {}");
    write(&r, "svc/tests/t.rs", "test");
    git(&r, &["add", "."]);
    (d, r)
}

fn run(cmd: &mut Command) -> Output {
    cmd.output().unwrap()
}

fn hash(r: &Path, extra: &[&str]) -> String {
    let o = run(bin().current_dir(r).arg("svc").args(extra));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

#[test]
fn hash_is_stable_sha256_hex() {
    let (_d, r) = repo();
    let h = hash(&r, &[]);
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(h, hash(&r, &[]));
    assert_eq!(hash(&r, &["-s", "12"]), h[..12]);
}

#[test]
fn subtree_content_changes_hash() {
    let (_d, r) = repo();
    let h = hash(&r, &[]);
    write(&r, "svc/a/f.rs", "fn main() { 1 }");
    assert_ne!(hash(&r, &[]), h);
}

#[test]
fn ancestor_lock_file_changes_hash_but_unrelated_file_does_not() {
    let (_d, r) = repo();
    let h = hash(&r, &[]);
    write(&r, "other.txt", "changed");
    assert_eq!(hash(&r, &[]), h);
    write(&r, "Cargo.lock", "lock2");
    assert_ne!(hash(&r, &[]), h);
}

#[test]
fn list_shows_subtree_and_ancestor_files() {
    let (_d, r) = repo();
    let o = run(bin().current_dir(&r).args(["svc", "-l"]));
    let out = String::from_utf8(o.stdout).unwrap();
    for f in ["./Cargo.toml", "./a/f.rs", "./tests/t.rs", "/Cargo.lock"] {
        assert!(out.lines().any(|l| l == f), "missing {f} in {out}");
    }
    assert!(!out.contains("other.txt"));
}

#[test]
fn no_defaults_drops_ancestor_files() {
    let (_d, r) = repo();
    let h = hash(&r, &["--no-defaults"]);
    write(&r, "Cargo.lock", "lock2");
    assert_eq!(hash(&r, &["--no-defaults"]), h);
}

#[test]
fn exclude_and_only_filter_subtree_files() {
    let (_d, r) = repo();
    let h = hash(&r, &["--exclude", "tests/"]);
    write(&r, "svc/tests/t.rs", "changed");
    assert_eq!(hash(&r, &["--exclude", "TESTS/"]), h);
    let h2 = hash(&r, &["--only", "a/"]);
    write(&r, "svc/Cargo.toml", "changed");
    assert_eq!(hash(&r, &["--only", "a/"]), h2);
}

#[test]
fn salt_and_build_param_change_hash() {
    let (_d, r) = repo();
    let h = hash(&r, &[]);
    let s = hash(&r, &["--salt", "x"]);
    let p = hash(&r, &["--build-param", "x"]);
    assert!(s != h && p != h && s != p);
}

#[test]
fn untracked_files_only_count_with_flag() {
    let (_d, r) = repo();
    let h = hash(&r, &[]);
    let hu = hash(&r, &["--untracked"]);
    write(&r, "svc/new.rs", "new");
    assert_eq!(hash(&r, &[]), h);
    assert_ne!(hash(&r, &["--untracked"]), hu);
}

#[cfg(unix)]
#[test]
fn exec_bit_changes_hash() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, r) = repo();
    let h = hash(&r, &[]);
    let p = r.join("svc/a/f.rs");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(hash(&r, &[]), h);
}

#[test]
fn manifest_ends_with_total_matching_plain_hash() {
    let (_d, r) = repo();
    let o = run(bin().current_dir(&r).args(["svc", "-m"]));
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(out.lines().any(|l| l.starts_with("./a/f.rs ")));
    assert_eq!(
        out.lines().last().unwrap(),
        format!("Total: {}", hash(&r, &[]))
    );
}

#[test]
fn moving_the_subtree_keeps_the_hash() {
    let (_d, r) = repo();
    let h = hash(&r, &["--no-defaults"]);
    git(&r, &["mv", "svc", "moved"]);
    let o = run(bin().current_dir(&r).args(["moved", "--no-defaults"]));
    assert_eq!(String::from_utf8(o.stdout).unwrap().trim(), h);
}

#[test]
fn config_file_supplies_root_filters_and_salt() {
    let (_d, r) = repo();
    write(
        &r,
        "hb.json",
        r#"{"inputRoot":"svc","exclude":["tests/"],"salt":"s","buildCmd":"x"}"#,
    );
    let via_cfg = run(bin().current_dir(&r).args(["-c", "hb.json"]));
    let direct = hash(&r, &["--exclude", "tests/", "--salt", "s"]);
    assert_eq!(String::from_utf8(via_cfg.stdout).unwrap().trim(), direct);
}

#[test]
fn not_a_git_repo_fails_cleanly() {
    let d = tempfile::tempdir().unwrap();
    let o = run(bin().arg(d.path()));
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error:"));
}

#[cfg(unix)]
mod build {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let (d, r) = repo();
        write(
            &r,
            "hb.json",
            r#"{"name":"svc","inputRoot":"svc","outputDir":"out",
                "buildCmd":"echo x >> ../builds.log && mkdir -p out && echo built > out/r.txt && chmod +x out/r.txt"}"#,
        );
        let arch = r.join("archive");
        (d, r, arch)
    }

    fn build(r: &Path, arch: &Path, envs: &[(&str, &str)]) -> Output {
        let mut c = bin();
        c.current_dir(r)
            .args(["-c", "hb.json", "--build", "--archive"])
            .arg(arch);
        for (k, v) in envs {
            c.env(k, v);
        }
        run(&mut c)
    }

    fn builds(r: &Path) -> usize {
        fs::read_to_string(r.join("builds.log"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    #[test]
    fn miss_builds_and_archives_then_hit_restores_without_building() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, r, arch) = setup();
        let o = build(&r, &arch, &[]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let h = String::from_utf8(o.stdout).unwrap().trim().to_string();
        assert!(arch.join(format!("svc_{h}.zip")).is_file());
        assert_eq!(builds(&r), 1);

        fs::remove_dir_all(r.join("svc/out")).unwrap();
        let o = build(&r, &arch, &[]);
        assert!(o.status.success());
        assert_eq!(builds(&r), 1, "second run must not build");
        let restored = r.join("svc/out/r.txt");
        assert_eq!(fs::read_to_string(&restored).unwrap().trim(), "built");
        assert_ne!(
            fs::metadata(&restored).unwrap().permissions().mode() & 0o111,
            0
        );
    }

    #[test]
    fn changed_source_misses_the_cache() {
        let (_d, r, arch) = setup();
        build(&r, &arch, &[]);
        write(&r, "svc/a/f.rs", "changed");
        build(&r, &arch, &[]);
        assert_eq!(builds(&r), 2);
    }

    #[test]
    fn failed_build_writes_no_archive_and_fails() {
        let (_d, r, arch) = setup();
        write(
            &r,
            "hb.json",
            r#"{"name":"svc","inputRoot":"svc","outputDir":"out","buildCmd":"false"}"#,
        );
        let o = build(&r, &arch, &[]);
        assert!(!o.status.success());
        let zips = fs::read_dir(&arch).map(|d| d.count()).unwrap_or(0);
        assert_eq!(zips, 0);
    }

    #[test]
    fn missing_output_dir_is_an_error_and_leaves_no_archive() {
        let (_d, r, arch) = setup();
        write(
            &r,
            "hb.json",
            r#"{"name":"svc","inputRoot":"svc","outputDir":"out","buildCmd":"true"}"#,
        );
        let o = build(&r, &arch, &[]);
        assert!(!o.status.success());
        assert_eq!(fs::read_dir(&arch).unwrap().count(), 0);
    }

    #[test]
    fn uploader_runs_after_build_with_zip_path() {
        let (_d, r, arch) = setup();
        let up = format!("cp [ZIP] {}/uploaded.zip", r.display());
        let o = build(&r, &arch, &[("HASHIBUILD_UPLOADER", &up)]);
        assert!(o.status.success());
        assert!(r.join("uploaded.zip").is_file());
    }

    #[test]
    fn failing_uploader_does_not_fail_the_build() {
        let (_d, r, arch) = setup();
        let o = build(&r, &arch, &[("HASHIBUILD_UPLOADER", "false")]);
        assert!(o.status.success());
    }

    #[test]
    fn fetcher_supplies_archive_on_local_miss_and_failure_falls_back_to_build() {
        let (_d, r, arch) = setup();
        let h = String::from_utf8(build(&r, &arch, &[]).stdout)
            .unwrap()
            .trim()
            .to_string();
        let remote = r.join("remote");
        fs::create_dir_all(&remote).unwrap();
        fs::rename(
            arch.join(format!("svc_{h}.zip")),
            remote.join(format!("svc_{h}.zip")),
        )
        .unwrap();
        fs::remove_dir_all(r.join("svc/out")).unwrap();

        let ok = format!("cp {}/[ZIP] [DEST]", remote.display());
        let o = build(&r, &arch, &[("HASHIBUILD_FETCHER", &ok)]);
        assert!(o.status.success());
        assert_eq!(builds(&r), 1, "fetched archive must prevent a rebuild");
        assert!(r.join("svc/out/r.txt").is_file());

        fs::remove_dir_all(&arch).unwrap();
        fs::remove_dir_all(r.join("svc/out")).unwrap();
        let o = build(&r, &arch, &[("HASHIBUILD_FETCHER", "false")]);
        assert!(o.status.success());
        assert_eq!(builds(&r), 2);
    }

    #[test]
    fn build_param_is_substituted_and_hashed() {
        let (_d, r, arch) = setup();
        write(
            &r,
            "hb.json",
            r#"{"name":"svc","inputRoot":"svc","outputDir":"out","buildCmd":"mkdir -p out && echo [BUILDPARAM] > out/p.txt"}"#,
        );
        let mut c = bin();
        c.current_dir(&r)
            .args(["-c", "hb.json", "-b", "--build-param", "prod", "--archive"])
            .arg(&arch);
        assert!(run(&mut c).status.success());
        assert_eq!(
            fs::read_to_string(r.join("svc/out/p.txt")).unwrap().trim(),
            "prod"
        );
    }

    #[test]
    fn vacuum_alone_cleans_archive() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("old.zip");
        fs::write(&old, "x").unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86400);
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(t)
            .unwrap();
        let o = run(bin().args(["--vacuum", "--archive"]).arg(d.path()));
        assert!(o.status.success());
        assert!(!old.exists());
    }

    #[test]
    fn build_without_archive_configured_fails_with_message() {
        let (_d, r, _a) = setup();
        let o = run(bin()
            .current_dir(&r)
            .env_remove("HASHIBUILD_ARCHIVE")
            .args(["-c", "hb.json", "-b"]));
        assert!(!o.status.success());
        assert!(String::from_utf8_lossy(&o.stderr).contains("HASHIBUILD_ARCHIVE"));
    }
}
