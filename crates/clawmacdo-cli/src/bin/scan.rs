use std::process::Command;
use std::path::PathBuf;

fn main(){
    let out = std::env::args().nth(1).unwrap_or("/tmp/openclaw_ubuntu_scan_$(date +%s).json".to_string());
    let status = Command::new("/bin/bash").arg("scripts/ubuntu_scan.sh").arg(&out).status().expect("failed to run scan");
    println!("scan script exit: {}", status);
}
