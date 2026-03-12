use std::process::Command;
use std::path::PathBuf;

fn main(){
    let args: Vec<String> = std::env::args().collect();
    let mut out = "/tmp/openclaw_scan_".to_string()+&format!("{}.json", chrono::Utc::now().timestamp());
    let mut target = "ubuntu".to_string();
    if args.len()>1 { target = args[1].clone(); }
    if args.len()>2 { out = args[2].clone(); }
    match target.as_str() {
        "ubuntu" => {
            let status = Command::new("/bin/bash").arg("scripts/ubuntu_scan.sh").arg(&out).status().expect("failed to run ubuntu scan");
            println!("ubuntu scan exit: {}", status);
        }
        "macos" => {
            let status = Command::new("/bin/bash").arg("scripts/macos_scan.sh").arg(&out).status().expect("failed to run macos scan");
            println!("macos scan exit: {}", status);
        }
        "windows" => {
            let status = Command::new("/bin/bash").arg("scripts/windows_scan_wrapper.sh").arg(&out).status().expect("failed to run windows wrapper");
            println!("windows scan wrapper exit: {}", status);
        }
        _ => println!("unknown target: {}", target)
    }
}
