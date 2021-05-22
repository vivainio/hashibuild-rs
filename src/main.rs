use serde::{Serialize, Deserialize};
use std::process::Command;
use std::fs::File;
use sha2::{Sha256, Digest};
use std::io::Write;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all="camelCase")]
struct AppConfig {
    name: String,
    exclude: Vec<String>,
    input_root: String,
    build_cmd: String,
    output_dirs: Vec<String>
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
    fn feed_file(&mut self, path: &str) {
        self.sha.write(path.as_bytes()).expect("name write failed??");
        let mut file = File::open(&path).expect("File not found");

        std::io::copy(&mut file, &mut self.sha).expect("copy failed");
    }

    fn result(self) -> String {
        let fin = &self.sha.finalize()[..];
        base64::encode(fin)
    }
}


fn parse_config(path: &str) -> AppConfig {
    let f = File::open(path).expect("config file not found");
    serde_json::from_reader(&f).expect("json parsing failed")

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
    let _tc = testconfig();
    dbg!(_tc);

}
