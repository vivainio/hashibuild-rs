use serde::{Serialize, Deserialize};
use std::process::Command;
use std::fs::File;
use sha2::{Sha256, Digest};
use std::io::Write;
use std::path::{Path};
use path_clean::clean;
use std::{env, fs};
use getopts::Options;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all="camelCase")]
struct AppConfig {
    name: String,
    input_root: String,
    build_cmd: String,
    output_dirs: Vec<String>,
    output_root: String,
    include: Vec<String>,
    exclude: Vec<String>,

}


#[cfg(test)]
fn testconfig() -> AppConfig {
    parse_config("test/testprj.json")
    //serde_json::from_str( )

}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt("c", "config", "Project file (json) to use", "PROJECT");
    opts.optflag("h", "help", "Show help");

    let matches = opts.parse(&args[1..]).unwrap();
    if matches.opt_present("h") {
        let brief = opts.usage(&program);
        println!("{}", &brief);
        return;
    }

    if matches.opt_present("c") {
        let val = matches.opt_str("c").unwrap();
        let ac = parse_config(&val);
        handle_project(&ac);
        return;
    }
}

fn git_ls_files(path: &str) -> String {
    let out =
        Command::new("git")
            .args(&["ls-files"])
            .current_dir(&path)
            .output()
            .expect("Failed to get files from git");
    String::from_utf8(out.stdout).expect("failed to parse file names")
}

struct PrjHasher {
    sha: Sha256
}

impl PrjHasher {
    fn new() -> PrjHasher {
        PrjHasher {
            sha: Sha256::new()
        }
    }
    fn feed_file(&mut self, relpath: &str, abspath: &str) {
        self.sha.write(relpath.as_bytes()).expect("name write failed??");
        let mut file = File::open(&abspath).expect("File not found");

        std::io::copy(&mut file, &mut self.sha).expect("copy failed");
    }

    fn result(self) -> String {
        let fin = &self.sha.finalize()[..];
        base32::encode(base32::Alphabet::RFC4648 {
            padding: false
        }, fin)
    }
}


fn parse_config(path: &str) -> AppConfig {
    let f = File::open(path).expect("config file not found");
    let mut config: AppConfig = serde_json::from_reader(&f).expect("json parsing failed");

    // fixup paths to be absolute


    let root_path = path_normalize(&path_join(path, ".."));

    config.input_root = path_join(&root_path, &config.input_root);
    config.output_root = path_join(&root_path, &config.output_root);
    config
}

fn starts_with_any(s: &str, tries: &Vec<String>) -> bool {
    for t in tries {
        if s.starts_with(t) {
            return true;
        }
    }
    false
}

fn collect_files_for_config(config: &AppConfig) -> String {
    let cont = git_ls_files(&config.input_root);
    let files = cont.lines();

    //files.filter(|l| l.)

    let mut hasher = PrjHasher::new();
    let root = Path::new(&config.input_root);
    let have_includes = config.include.len() > 0;
    for f in files {
        let abs = root.join(&f);
        let ll = path_normalize(&abs.to_string_lossy());

        if starts_with_any(f, &config.exclude) {
            println!("Skipping exclude {}",&f);
            continue;
        }

        if have_includes && !starts_with_any(f, &config.include) {
            continue;
        }

        hasher.feed_file(&f, &ll);
    }
    hasher.result()
}

fn path_normalize(path_rel: &str) -> String {
    let canon = Path::new(path_rel).canonicalize().unwrap();
    let path = canon.to_string_lossy();

    clean(&path.replace("\\\\?\\", "").replace("\\", "/"))
}

fn path_join(a: &str, b: &str) -> String {
    clean(&format!("{}/{}", &a,&b))
}

fn run_in_shell(cmd: &str, cwd: &str) {
    let _exit = Command::new("cmd.exe")
        .args(["/C", cmd])
        .current_dir(cwd)
        .status()
        .expect("Shell command failed");
}

fn zip_output(root_path: &str, paths: &Vec<String>, zip_file: &str) {
    let mut cmd =
        Command::new("c:/bin/7za.exe");
    cmd.args(&["a", "-y", "-r", zip_file]);
    for p in paths {
        cmd.arg(&p);
    }
    cmd.current_dir(root_path);
    cmd.status().expect("Running 7za failed");
}

fn unzip_to_output(root_path: &str, zip_file: &str) {
    Command::new("c:/bin/7za.exe")
        .args(&["-y", "x", zip_file])
        .current_dir(&root_path)
        .status().expect("Running unzip failed");
}

fn discover_archive(config: &AppConfig, checksum: &str) -> (bool, String) {
    let archive = env::var("HASHIBUILD_ARCHIVE").unwrap_or_else(|_|"c:/t/hbcache".into());
    let zipname = format!("{}/{}_{}.zip", archive, config.name, checksum);
    (fs::metadata(&zipname).is_ok(), zipname)
}

fn handle_project(config: &AppConfig) {
    // count the hash

    let checksum = collect_files_for_config(&config);
    let (found, arc) = discover_archive(&config, &checksum);
    if !found {
        run_in_shell(&config.build_cmd, &config.input_root);
        zip_output(&config.output_root, &config.output_dirs, &arc);
    } else {
        unzip_to_output(&config.output_root, &arc);
    }



    // check the archive
    // build if needed
    // zip it up

    // or just unzip

}



#[test]
fn test_git() {
    let files = git_ls_files(".");
    dbg!(&files);
}

#[test]
fn test_get_checksums() {
    let mut h = PrjHasher::new();
    h.feed_file("", "Cargo.lock");
    h.feed_file("", "Cargo.toml");
    let res = h.result();
    dbg!(res);
}

#[test]
fn test_path() {
    let mut own = "c:/temp/a".to_owned();
    let s = path_join(&mut own, "../c");
    assert!(s == "c:/temp/c")
}
#[test]
fn test_config() {
    let tc = testconfig();
    dbg!(&tc);
    collect_files_for_config(&tc);
}

#[test]
fn test_discover_archive() {
    let tc= testconfig();
    let (_found, _path) = discover_archive(&tc, "crc");
    assert_eq!(_found, false);
    assert_eq!(_path, "c:/t/hbcache/hashibuildtest_crc.zip")
}


#[test]
fn run_build_command() {
    let tc= testconfig();
    run_in_shell(&tc.build_cmd, &tc.input_root);
    zip_output(&tc.output_root, &tc.output_dirs, "c:/t/test.zip")
}

#[test]
fn handle_full_build() {
    let tc= testconfig();
    handle_project(&tc);
}
