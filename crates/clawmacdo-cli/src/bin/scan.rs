use std::process::Command;
use std::env;

fn usage(){
    eprintln!("Usage: scan [--target=ubuntu|macos|windows|config|all] [--out=/path/to/out.json]");
}

fn main(){
    let mut target = String::from("ubuntu");
    let mut out = String::from("/tmp/openclaw_scan.json");
    for arg in env::args().skip(1) {
        if arg.starts_with("--target=") { target = arg[9..].to_string(); }
        else if arg.starts_with("--out=") { out = arg[6..].to_string(); }
        else if arg=="--help" { usage(); return; }
        else { eprintln!("Unknown arg: {}", arg); usage(); return; }
    }
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
        "config" => {
            let status = Command::new("/bin/bash").arg("scripts/openclaw_config_scan.sh").arg(&out).status().expect("failed to run config scan");
            println!("config scan exit: {}", status);
        }
        "all" => {
            let status = Command::new("/bin/bash").arg("scripts/run_all_scans.sh").status().expect("failed to run all scans");
            println!("run_all_scans exit: {}", status);
        }
        _ => { eprintln!("unknown target: {}", target); usage(); }
    }
}
