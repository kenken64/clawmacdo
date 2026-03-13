use clap::Parser;
use std::process::Command;

#[derive(Parser, Debug)]
#[command(name="scan", about="Run security scans", version)]
struct Args{
    /// Target to scan: ubuntu|macos|windows|config|all
    #[arg(long, default_value_t = String::from("ubuntu"))]
    target: String,
    /// Output file path (for single-target scans)
    #[arg(long)]
    out: Option<String>,
}

fn main(){
    let args = Args::parse();
    let out = args.out.unwrap_or_else(|| format!("/tmp/openclaw_scan_{}.json", chrono::Utc::now().timestamp()));
    let format = std::env::var("FORMAT").unwrap_or_else(|_| String::from("json"));
    match args.target.as_str() {
        "ubuntu" => { let status = Command::new("/bin/bash").arg("scripts/ubuntu_scan.sh").arg(&out).status().expect("failed"); println!("ubuntu scan exit: {}", status); }
        "macos" => { let status = Command::new("/bin/bash").arg("scripts/macos_scan.sh").arg(&out).status().expect("failed"); println!("macos scan exit: {}", status); }
        "windows" => { let status = Command::new("/bin/bash").arg("scripts/windows_scan_wrapper.sh").arg(&out).status().expect("failed"); println!("windows scan exit: {}", status); }
        "config" => { let status = Command::new("/bin/bash").arg("scripts/openclaw_config_scan.sh").arg(&out).status().expect("failed"); println!("config scan exit: {}", status); }
        "all" => { let status = Command::new("/bin/bash").arg("scripts/run_all_scans.sh").status().expect("failed"); println!("run_all_scans exit: {}", status); }
        _ => eprintln!("unknown target: {}", args.target)
    }
    if format!="json" && args.target!="all" {
        let fmt_status = Command::new("/bin/bash").arg("scripts/format_outputs.sh").arg(&out).arg(&format).arg(&out).status().expect("format failed");
        println!("format exit: {}", fmt_status);
    }
}
