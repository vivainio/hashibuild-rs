use serde::{Serialize, Deserialize};
use std::process::Command;
use std::fs::File;
use sha2::{Sha256, Digest};
use std::io::Write;
use std::path::{Path};

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

fn testconfig() -> AppConfig {
    parse_config("test/testprj.json")
    //serde_json::from_str( )

}
fn main() {

    let ac = testconfig();
    let as_str = serde_json::to_string(&ac).unwrap();
    dbg!(&as_str);
    let parsed2: AppConfig = serde_json::from_str(&as_str).unwrap();
    dbg!(parsed2);
    //println!("Hello, world!");
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
        base64::encode(fin)
    }
}


fn parse_config(path: &str) -> AppConfig {
    let f = File::open(path).expect("config file not found");
    let mut config: AppConfig = serde_json::from_reader(&f).expect("json parsing failed");

    // fixup paths to be absolute

    let root_path = Path::new(path).parent().unwrap();

    config.input_root = root_path.join(config.input_root).canonicalize().unwrap().to_string_lossy().into();
    config.output_root = root_path.join(config.output_root).canonicalize().unwrap().to_string_lossy().into();
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

fn collect_files_for_config(config: &AppConfig) {
    let cont = git_ls_files(&config.input_root);
    let files = cont.lines();

    //files.filter(|l| l.)

    let mut hasher = PrjHasher::new();
    let root = Path::new(&config.input_root);
    let have_includes = config.include.len() > 0;
    for f in files {
        let abs = root.join(&f);
        let ll = normalize_path(&abs.to_string_lossy());
        dbg!(&ll);


        if starts_with_any(f, &config.exclude) {
            println!("Skipping exclude {}",&f);
            continue;
        }

        if have_includes && !starts_with_any(f, &config.include) {
            continue;
        }


        hasher.feed_file(&f, &ll);
    }
}

fn normalize_path(path: &str) -> String
{
    path.replace("\\\\?\\", "").replace("\\", "/")
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
fn test_config() {
    let tc = testconfig();
    dbg!(&tc);
    collect_files_for_config(&tc);

}

