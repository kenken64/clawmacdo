// CLI scaffold: security scan command
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct ScanSecurity {
    #[clap(long, default_value = "all")]
    pub target: String,
    #[clap(long)]
    pub host: Option<String>,
    #[clap(long)]
    pub out: Option<PathBuf>,
}

#[allow(dead_code)]
pub fn run(args: ScanSecurity) -> anyhow::Result<()> {
    println!(
        "TODO: implement security scan. target={} host={:?} out={:?}",
        args.target, args.host, args.out
    );
    Ok(())
}
