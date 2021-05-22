use serde::{Serialize, Deserialize};
use std::process::Command;

#[derive(Serialize, Deserialize, Debug)]

struct AppConfig {
    name: String,
    exclude: Vec<String>
}

fn main() {

    let ac = AppConfig {
        name: "test".to_string(),
        exclude: vec!["one".to_string(), "two".to_string()]
    };
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



#[test]
fn test_git() {
    let files = git_ls_files(".");
    dbg!(&files);
}
