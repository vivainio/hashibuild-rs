use serde::{Serialize, Deserialize};
use std::process::Command;
use std::fs::File;
use sha2::{Sha256, Digest};
use std::io::Write;

#[derive(Serialize, Deserialize, Debug)]

struct AppConfig {
    name: String,
    exclude: Vec<String>,
    inputroot: String,
    build_cmd: String
    output_dirs: String
}

fn testconfig() -> AppConfig {
    AppConfig {
        name: "test".to_string(),
        exclude: vec!["one".to_string(), "two".to_string()],
        inputroot: ".".to_string()
    }
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
    fn feed_file(&mut self, path: &str) {
        self.sha.write(path.as_bytes());
        let mut file = File::open(&path).expect("File not found");

        std::io::copy(&mut file, &mut self.sha).expect("copy failed");
    }

    fn result(self) -> String {
        let fin = &self.sha.finalize()[..];
        base64::encode(fin)
    }
}


fn parse_config(path: &string) {

}
#[test]
fn test_git() {
    let files = git_ls_files(".");
    dbg!(&files);
}

#[test]
fn test_get_checksums() {
    let mut h = PrjHasher::new();
    h.feed_file("Cargo.lock");
    h.feed_file("Cargo.toml");
    let res = h.result();
    dbg!(res);
}

#[test]
fn test_config() {
    parse_config("testdata.jsos")
}
