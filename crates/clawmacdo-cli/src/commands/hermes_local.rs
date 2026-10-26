//! Install Hermes Agent natively on this computer (macOS or Windows) and remove it again.
//!
//! By default the software comes from clawmacdo's pre-built bundle, so nothing compiles
//! here; `--from-source` runs the pinned upstream installer instead. Either way clawmacdo
//! writes the same AI/Telegram configuration as `hermes-provision`, registers the gateway
//! service through `hermes gateway install`, and records what it changed so
//! `hermes-local-uninstall` only removes installs it created.

use super::hermes_config::{
    build_config_content, build_env_content, build_opencode_config_content,
    normalized_openrouter_model, resolve_ai_provider, validate_telegram_bot_token,
    verify_telegram_bot_token, AiProvider, HermesAgentSettings, TelegramVerification,
    TerminalBackend,
};
use super::hermes_local_bundle::{self as bundle, BundleManifest, ExpectedBundle};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use clawmacdo_core::config;
use dialoguer::Confirm;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;

const TOTAL_STEPS: u32 = 6;
const RECORD_FILE: &str = "hermes-local.json";
const STATE_DIR: &str = "hermes-local";
const OPENCODE_INSTALLER_URL: &str = "https://opencode.ai/v2/install";
const WINDOWS_TASK_NAME: &str = "Hermes_Gateway";
const LAUNCH_AGENT_PREFIXES: [&str; 2] =
    ["ai.hermes.gateway", "io.nousresearch.hermes-agent.gateway"];
const LOCAL_TERMINAL_WARNING: &str = "WARNING: terminal backend 'local' lets the agent run shell commands on this computer with your user's permissions. Use --terminal-backend docker to run them in a sandbox.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalPlatform {
    MacOs,
    Windows,
}

impl LocalPlatform {
    fn current() -> Result<Self> {
        if cfg!(target_os = "macos") {
            Ok(LocalPlatform::MacOs)
        } else if cfg!(target_os = "windows") {
            Ok(LocalPlatform::Windows)
        } else {
            bail!(
                "Local Hermes installs support macOS and Windows. \
                 For a Linux server, use `clawmacdo hermes-provision` (AWS Lightsail)."
            )
        }
    }

    fn label(self) -> &'static str {
        match self {
            LocalPlatform::MacOs => "macOS",
            LocalPlatform::Windows => "Windows",
        }
    }

    fn installer_file(self) -> &'static str {
        match self {
            LocalPlatform::MacOs => "install.sh",
            LocalPlatform::Windows => "install.ps1",
        }
    }

    /// The pre-built bundle's platform name (Apple Silicon and x64 builds only).
    fn bundle_platform(self) -> &'static str {
        match self {
            LocalPlatform::MacOs => "macos-arm64",
            LocalPlatform::Windows => "windows-x64",
        }
    }
}

/// How Hermes got onto this computer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallMethod {
    /// clawmacdo's pre-built bundle; nothing was compiled here.
    Bundle,
    /// Upstream's installer, which builds Hermes from source. Records written before
    /// bundles existed are source installs.
    #[default]
    Source,
}

/// Upstream versions and checksums pinned in the Hermes bundle manifest.
#[derive(serde::Deserialize)]
struct Pins {
    repository: String,
    release: String,
    hermes_version: String,
    hermes_commit: String,
    installer_sha256: String,
    installer_ps1_sha256: String,
    opencode_version: String,
    opencode_installer_sha256: String,
}

impl Pins {
    fn load() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../../../../scripts/hermes-bundle/bundle.json"
        ))
        .context("Pinned Hermes bundle manifest is invalid")
    }

    fn installer_url(&self, platform: LocalPlatform) -> String {
        format!(
            "https://raw.githubusercontent.com/NousResearch/hermes-agent/{}/scripts/{}",
            self.hermes_commit,
            platform.installer_file()
        )
    }

    fn installer_sha256(&self, platform: LocalPlatform) -> &str {
        match platform {
            LocalPlatform::MacOs => &self.installer_sha256,
            LocalPlatform::Windows => &self.installer_ps1_sha256,
        }
    }

    fn expected_bundle(&self, platform: LocalPlatform) -> ExpectedBundle<'_> {
        ExpectedBundle {
            release: &self.release,
            platform: platform.bundle_platform(),
            hermes_commit: &self.hermes_commit,
            hermes_version: &self.hermes_version,
        }
    }

    fn bundle_url(&self, platform: LocalPlatform) -> String {
        format!(
            "https://github.com/{}/releases/download/{}/{}",
            self.repository,
            self.release,
            self.expected_bundle(platform).filename()
        )
    }
}

struct LocalPaths {
    home: PathBuf,
    hermes_home: PathBuf,
    state_dir: PathBuf,
    record: PathBuf,
}

impl LocalPaths {
    fn resolve(platform: LocalPlatform) -> Result<Self> {
        let home = dirs::home_dir().context("Could not determine your home directory")?;
        let hermes_home = hermes_home_for(
            platform,
            std::env::var_os("HERMES_HOME"),
            &home,
            std::env::var_os("LOCALAPPDATA"),
        )?;
        let app_dir = config::app_dir()?;
        Ok(LocalPaths {
            home,
            hermes_home,
            state_dir: app_dir.join(STATE_DIR),
            record: app_dir.join(RECORD_FILE),
        })
    }

    fn opencode_dir(&self) -> PathBuf {
        self.home.join(".opencode")
    }

    fn opencode_config_dir(&self) -> PathBuf {
        self.home.join(".config").join("opencode")
    }
}

/// Mirror upstream's default: `HERMES_HOME`, else `~/.hermes` (macOS) or
/// `%LOCALAPPDATA%\hermes` (Windows).
fn hermes_home_for(
    platform: LocalPlatform,
    hermes_home_env: Option<OsString>,
    home: &Path,
    local_app_data: Option<OsString>,
) -> Result<PathBuf> {
    if let Some(value) = hermes_home_env.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    match platform {
        LocalPlatform::MacOs => Ok(home.join(".hermes")),
        LocalPlatform::Windows => local_app_data
            .filter(|value| !value.is_empty())
            .map(|value| PathBuf::from(value).join("hermes"))
            .context("LOCALAPPDATA is not set; cannot locate the Hermes home folder"),
    }
}

fn launcher_candidates(platform: LocalPlatform, home: &Path, hermes_home: &Path) -> Vec<PathBuf> {
    match platform {
        LocalPlatform::MacOs => vec![
            home.join(".local").join("bin").join("hermes"),
            hermes_home
                .join("hermes-agent")
                .join(".hermes")
                .join("bin")
                .join("hermes"),
            hermes_home
                .join("hermes-agent")
                .join("venv")
                .join("bin")
                .join("hermes"),
        ],
        LocalPlatform::Windows => vec![
            hermes_home.join("bin").join("hermes.exe"),
            hermes_home.join("bin").join("hermes.cmd"),
        ],
    }
}

fn find_launcher(platform: LocalPlatform, paths: &LocalPaths) -> Option<PathBuf> {
    launcher_candidates(platform, &paths.home, &paths.hermes_home)
        .into_iter()
        .find(|path| path.is_file())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// A program and its arguments, kept separate from execution so it can be tested.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}

impl CommandSpec {
    fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

fn installer_command(
    platform: LocalPlatform,
    script: &Path,
    commit: &str,
    computer_use: bool,
) -> CommandSpec {
    let script = script.display().to_string();
    let (program, mut args) = match platform {
        LocalPlatform::MacOs => (
            "bash",
            vec![
                script,
                "--non-interactive".into(),
                "--commit".into(),
                commit.into(),
            ],
        ),
        LocalPlatform::Windows => (
            "powershell.exe",
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script,
                "-NonInteractive".into(),
                "-Commit".into(),
                commit.into(),
            ],
        ),
    };
    if !computer_use {
        args.push(
            match platform {
                LocalPlatform::MacOs => "--skip-computer-use",
                LocalPlatform::Windows => "-SkipComputerUse",
            }
            .into(),
        );
    }
    CommandSpec {
        program: program.into(),
        args,
    }
}

/// `hermes <args>`; Windows `.cmd` launchers must run through `cmd /C`.
fn hermes_command(launcher: &Path, args: &[&str]) -> CommandSpec {
    let is_cmd = launcher
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"));
    let launcher = launcher.display().to_string();
    let args = args.iter().map(|arg| arg.to_string());
    if is_cmd {
        CommandSpec {
            program: "cmd".into(),
            args: ["/C".to_string(), launcher]
                .into_iter()
                .chain(args)
                .collect(),
        }
    } else {
        CommandSpec {
            program: launcher,
            args: args.collect(),
        }
    }
}

fn uninstall_args(keep_data: bool) -> Vec<&'static str> {
    if keep_data {
        vec!["uninstall", "--yes"]
    } else {
        vec!["uninstall", "--yes", "--full"]
    }
}

/// What `hermes uninstall` should have deleted: the whole Hermes home, or only the
/// code checkout when data is kept (both installers default it to `<home>/hermes-agent`).
fn uninstall_target(hermes_home: &Path, keep_data: bool) -> PathBuf {
    if keep_data {
        hermes_home.join("hermes-agent")
    } else {
        hermes_home.to_path_buf()
    }
}

/// Never recursively delete the user's home folder, one of its ancestors, or a root.
fn safe_to_delete(target: &Path, home: &Path) -> bool {
    target.parent().is_some() && !home.starts_with(target)
}

/// Upstream's uninstall can exit 0 but leave files behind on Windows: read-only git
/// pack files and the renamed launcher that was still running. Once `hermes` has
/// exited nothing holds them, so finish the removal, retrying briefly for file locks.
async fn remove_leftovers(target: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::remove_dir_all(target) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) if attempt >= 3 => return Err(err),
            Err(_) => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_secs(attempt)).await;
            }
        }
    }
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = hex::encode(Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        bail!("Checksum mismatch: expected SHA-256 {expected}, got {actual}. The download was not run.");
    }
    Ok(())
}

async fn download_verified(url: &str, expected_sha256: &str, dest: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let bytes = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("Failed to download {url}"))?
        .bytes()
        .await
        .with_context(|| format!("Failed to read {url}"))?;
    verify_sha256(&bytes, expected_sha256).with_context(|| format!("Refusing {url}"))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &bytes).with_context(|| format!("Failed to save {}", dest.display()))
}

/// Write a file only the current user can read (0600 on Unix; Windows keeps the
/// per-user ACL of the profile folder).
fn write_private(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::write(path, contents)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Copy `path` into `backup_dir/name` before it is overwritten.
fn backup_existing(path: &Path, backup_dir: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    let dest = backup_dir.join(name);
    let contents = std::fs::read(path)
        .with_context(|| format!("Failed to read {} for backup", path.display()))?;
    write_private(&dest, "")?;
    std::fs::write(&dest, contents)
        .with_context(|| format!("Failed to back up {}", path.display()))?;
    Ok(Some(dest))
}

/// A file clawmacdo wrote, and where its previous version was saved.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WrittenFile {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
}

/// Saved at `~/.clawmacdo/hermes-local.json`; uninstall only acts on installs it describes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LocalInstallRecord {
    pub platform: LocalPlatform,
    pub hermes_home: PathBuf,
    pub launcher: PathBuf,
    pub hermes_version: String,
    pub hermes_commit: String,
    pub ai_model: String,
    pub terminal_backend: TerminalBackend,
    pub service_installed: bool,
    pub opencode_binary_installed: bool,
    pub written_files: Vec<WrittenFile>,
    pub backup_dir: Option<PathBuf>,
    pub installed_at: DateTime<Utc>,
    #[serde(default)]
    pub install_method: InstallMethod,
    /// The pre-built bundle release, for bundle installs.
    #[serde(default)]
    pub bundle_release: Option<String>,
}

fn load_record(path: &Path) -> Result<Option<LocalInstallRecord>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map(Some)
            .with_context(|| format!("Install record {} is corrupt", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Failed to read {}", path.display())),
    }
}

/// Signs that Hermes is already installed, so a new install would overwrite it.
fn existing_install_findings(
    paths: &LocalPaths,
    record: Option<&LocalInstallRecord>,
    hermes_on_path: Option<PathBuf>,
    gateway_service: Option<String>,
) -> Vec<String> {
    let mut findings = Vec::new();
    if let Some(record) = record {
        findings.push(format!(
            "clawmacdo installed Hermes here on {} (record: {})",
            record.installed_at.format("%Y-%m-%d %H:%M UTC"),
            paths.record.display()
        ));
    }
    for name in ["config.yaml", ".env"] {
        let path = paths.hermes_home.join(name);
        if path.is_file() {
            findings.push(format!("Hermes configuration exists: {}", path.display()));
        }
    }
    if let Some(path) = hermes_on_path {
        findings.push(format!("`hermes` is already on PATH: {}", path.display()));
    }
    if let Some(service) = gateway_service {
        findings.push(format!("A Hermes gateway service is registered: {service}"));
    }
    findings
}

async fn probe(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

async fn find_gateway_service(platform: LocalPlatform, home: &Path) -> Option<String> {
    match platform {
        LocalPlatform::MacOs => std::fs::read_dir(home.join("Library").join("LaunchAgents"))
            .ok()?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        LAUNCH_AGENT_PREFIXES
                            .iter()
                            .any(|prefix| name.starts_with(prefix))
                    })
            })
            .map(|path| path.display().to_string()),
        LocalPlatform::Windows => probe("schtasks", &["/Query", "/TN", WINDOWS_TASK_NAME])
            .await
            .then(|| format!("Scheduled Task {WINDOWS_TASK_NAME}")),
    }
}

fn hermes_on_path(platform: LocalPlatform) -> Option<PathBuf> {
    match platform {
        LocalPlatform::MacOs => find_on_path("hermes"),
        LocalPlatform::Windows => find_on_path("hermes.exe").or_else(|| find_on_path("hermes.cmd")),
    }
}

/// npm's cache directory: `npm_config_cache` when set, otherwise `~/.npm`.
#[cfg(unix)]
fn npm_cache_dir(home: &Path, configured: Option<OsString>) -> PathBuf {
    configured
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".npm"))
}

/// First entry under `root` not owned by `uid`, such as files left by an earlier `sudo npm`.
#[cfg(unix)]
fn first_foreign_owned(root: &Path, uid: u32) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.uid() != uid {
            return Some(path);
        }
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                pending.extend(entries.flatten().map(|entry| entry.path()));
            }
        }
    }
    None
}

async fn check_prerequisites(
    platform: LocalPlatform,
    backend: TerminalBackend,
    #[cfg_attr(not(unix), allow(unused_variables))] home: &Path,
    #[cfg_attr(not(unix), allow(unused_variables))] from_source: bool,
) -> Result<()> {
    match platform {
        LocalPlatform::MacOs => {
            // `xcode-select -p` checks for git without triggering Apple's install dialog.
            let homebrew_git =
                find_on_path("git").is_some_and(|path| !path.starts_with("/usr/bin"));
            if !homebrew_git && !probe("xcode-select", &["-p"]).await {
                bail!(
                    "Hermes needs git from Apple's Command Line Tools. \
                     Run `xcode-select --install`, finish the dialog, then retry."
                );
            }
            // Upstream's installer updates an existing checkout before its `npm ci` step,
            // so a cache npm cannot write to would fail with Hermes half-updated.
            // The pre-built bundle never runs npm.
            #[cfg(unix)]
            if from_source {
                let cache = npm_cache_dir(
                    home,
                    std::env::var_os("npm_config_cache")
                        .or_else(|| std::env::var_os("NPM_CONFIG_CACHE")),
                );
                // SAFETY: getuid has no preconditions and cannot fail.
                let uid = unsafe { libc::getuid() };
                let cacache = cache.join("_cacache");
                let foreign =
                    tokio::task::spawn_blocking(move || first_foreign_owned(&cacache, uid))
                        .await
                        .context("npm cache ownership check panicked")?;
                if let Some(path) = foreign {
                    bail!(
                        "npm's cache contains files owned by another user, usually root from an \
                         earlier `sudo npm`: {}. The Hermes installer runs `npm ci`, which fails on \
                         them with EACCES partway through the install. Nothing was changed. Fix the \
                         ownership with `sudo chown -R $(id -u):$(id -g) \"{}\"`, then try again.",
                        path.display(),
                        cache.display()
                    );
                }
            }
        }
        LocalPlatform::Windows => {
            if find_on_path("powershell.exe").is_none() {
                bail!("Windows PowerShell (powershell.exe) was not found on PATH.");
            }
        }
    }
    if backend == TerminalBackend::Docker && !probe("docker", &["info"]).await {
        bail!(
            "--terminal-backend docker needs Docker Desktop installed and running \
             (`docker info` failed). Start Docker, or use --terminal-backend local."
        );
    }
    Ok(())
}

/// Where progress goes: stdout (stderr in `--json` mode so stdout stays valid JSON),
/// plus the web UI's progress channel when one is attached.
struct Reporter {
    json: bool,
    tx: Option<mpsc::UnboundedSender<String>>,
}

impl Reporter {
    fn say(&self, message: &str) {
        if self.json {
            eprintln!("{message}");
        } else {
            println!("{message}");
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(message.to_string());
        }
    }
}

/// Drop ANSI colour codes and carriage-return progress redraws from installer output.
fn clean_output_line(line: &str) -> String {
    let line = line.rsplit('\r').next().unwrap_or_default();
    let mut cleaned = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for code in chars.by_ref() {
                    if code.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        cleaned.push(ch);
    }
    cleaned.trim_end().to_string()
}

async fn forward_lines<R: tokio::io::AsyncRead + Unpin>(stream: Option<R>, out: &Reporter) {
    let Some(stream) = stream else {
        return;
    };
    let mut lines = tokio::io::BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = clean_output_line(&line);
        if !line.trim().is_empty() {
            out.say(&format!("    {line}"));
        }
    }
}

async fn run_command(spec: &CommandSpec, out: &Reporter, what: &str) -> Result<()> {
    run_prepared(spec.command(), spec, out, what).await
}

async fn run_prepared(
    mut command: Command,
    spec: &CommandSpec,
    out: &Reporter,
    what: &str,
) -> Result<()> {
    command.stdin(Stdio::null());
    let status = if out.tx.is_some() {
        // The web UI cannot see inherited output, so stream it line by line.
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .with_context(|| format!("Failed to start {what} ({})", spec.program))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        tokio::join!(forward_lines(stdout, out), forward_lines(stderr, out));
        child.wait().await?
    } else {
        if out.json {
            command.stdout(Stdio::from(std::io::stderr()));
        }
        command
            .status()
            .await
            .with_context(|| format!("Failed to start {what} ({})", spec.program))?
    };
    if !status.success() {
        bail!("{what} failed ({status}). Command: {}", spec.display());
    }
    Ok(())
}

pub struct HermesLocalInstallParams {
    pub settings: HermesAgentSettings,
    pub install_opencode: bool,
    pub computer_use: bool,
    pub install_service: bool,
    pub force: bool,
    /// Build with upstream's installer instead of using the pre-built bundle.
    pub from_source: bool,
    /// A local bundle archive (with its `.json` manifest beside it), for CI verification.
    pub bundle_archive: Option<PathBuf>,
    pub dry_run: bool,
    pub json: bool,
    /// Web UI progress channel; when set, the CLI summary is not printed.
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
}

/// What a local install did; `record` is `None` for a dry run.
#[cfg_attr(not(feature = "web-ui"), allow(dead_code))]
pub struct LocalInstallOutcome {
    pub record: Option<LocalInstallRecord>,
    pub telegram_bot: Option<String>,
    pub opencode: Option<String>,
}

/// Whether this computer can take a local install, and the current clawmacdo record.
#[cfg(feature = "web-ui")]
#[derive(Debug, serde::Serialize)]
pub struct LocalStatus {
    pub supported: bool,
    pub reason: Option<String>,
    pub platform: Option<LocalPlatform>,
    pub record: Option<LocalInstallRecord>,
}

/// Why this operating system cannot take a local install, if it cannot.
#[cfg(feature = "web-ui")]
pub fn platform_block_reason() -> Option<String> {
    LocalPlatform::current().err().map(|err| err.to_string())
}

#[cfg(feature = "web-ui")]
pub fn local_install_status() -> Result<LocalStatus> {
    let platform = match LocalPlatform::current() {
        Ok(platform) => platform,
        Err(err) => {
            return Ok(LocalStatus {
                supported: false,
                reason: Some(err.to_string()),
                platform: None,
                record: None,
            })
        }
    };
    let paths = LocalPaths::resolve(platform)?;
    Ok(LocalStatus {
        supported: true,
        reason: None,
        platform: Some(platform),
        record: load_record(&paths.record)?,
    })
}

struct OpenCodePlan {
    api_key: String,
    model: String,
}

/// Upstream's install tail, run in the relocated checkout: it publishes the `hermes`
/// command for this location (and on Windows adds it to the user PATH), syncs bundled
/// skills and migrates config. The bundle's products are already built.
fn completion_command(hermes_home: &Path, manifest: &BundleManifest) -> Result<CommandSpec> {
    let python = hermes_home.join(bundle::software_path(&manifest.store_python)?);
    Ok(CommandSpec {
        program: python.display().to_string(),
        args: vec![
            "-I".into(),
            "-B".into(),
            "-X".into(),
            "utf8".into(),
            "hermes_cli/source_completion.py".into(),
            "--source".into(),
            hermes_home.join("hermes-agent").display().to_string(),
        ],
    })
}

/// Steps 2 and 3 for the pre-built bundle: download and verify it, move any existing
/// Hermes software aside, extract and relocate the bundle, then publish the command.
/// A failure restores the previous software.
async fn install_from_bundle(
    params: &HermesLocalInstallParams,
    platform: LocalPlatform,
    paths: &LocalPaths,
    pins: &Pins,
    software_backup: &Path,
    out: &Reporter,
) -> Result<()> {
    out.say(&format!(
        "\n[Step 2/{TOTAL_STEPS}] Downloading the pre-built Hermes bundle..."
    ));
    let expected = pins.expected_bundle(platform);
    let (archive, manifest, downloaded) = match &params.bundle_archive {
        Some(archive) => {
            let manifest = bundle::read_local_manifest(archive)?;
            bundle::validate_manifest(&manifest, &expected)?;
            out.say(&format!("  Using {}", archive.display()));
            (archive.clone(), manifest, false)
        }
        None => {
            let url = pins.bundle_url(platform);
            let manifest =
                bundle::fetch_manifest(&format!("{url}.json"), platform.bundle_platform()).await?;
            bundle::validate_manifest(&manifest, &expected)?;
            out.say(&format!("  {url} ({} MiB)", manifest.size / (1024 * 1024)));
            let archive = paths.state_dir.join("downloads").join(expected.filename());
            let say = |line: &str| out.say(line);
            bundle::download_archive(&url, &archive, &manifest, &say).await?;
            (archive, manifest, true)
        }
    };

    out.say(&format!(
        "\n[Step 3/{TOTAL_STEPS}] Installing the pre-built Hermes bundle..."
    ));
    let result = place_bundle(platform, paths, &archive, &manifest, software_backup, out).await;
    if downloaded {
        let _ = std::fs::remove_file(&archive);
    }
    result
}

async fn place_bundle(
    platform: LocalPlatform,
    paths: &LocalPaths,
    archive: &Path,
    manifest: &BundleManifest,
    software_backup: &Path,
    out: &Reporter,
) -> Result<()> {
    let moved = bundle::move_existing_software(&paths.hermes_home, software_backup)?;
    if !moved.is_empty() {
        out.say(&format!(
            "  Moved the existing Hermes software ({}) to {}",
            moved.join(", "),
            software_backup.display()
        ));
    }
    out.say("  Verifying the checksum and extracting...");
    let extracted = {
        let (archive, manifest) = (archive.to_path_buf(), manifest.clone());
        let (hermes_home, home) = (paths.hermes_home.clone(), paths.home.clone());
        let windows = platform == LocalPlatform::Windows;
        tokio::task::spawn_blocking(move || {
            bundle::install_bundle(&archive, &manifest, &hermes_home, &home, windows)
        })
        .await
        .context("Hermes bundle extraction stopped unexpectedly")?
    };
    let rollback = || {
        for root in bundle::ROOTS {
            if !moved.iter().any(|done| done == root) {
                let _ = std::fs::remove_dir_all(paths.hermes_home.join(root));
            }
        }
        bundle::restore_existing_software(&paths.hermes_home, software_backup, &moved);
    };
    if let Err(err) = extracted {
        bundle::restore_existing_software(&paths.hermes_home, software_backup, &moved);
        return Err(err);
    }
    out.say(&format!(
        "  Extracted into {} and relocated for this computer.",
        paths.hermes_home.display()
    ));

    let completion = completion_command(&paths.hermes_home, manifest)?;
    let mut command = completion.command();
    command
        .current_dir(paths.hermes_home.join("hermes-agent"))
        .env("HERMES_HOME", &paths.hermes_home)
        .env("UV_NO_CONFIG", "1");
    if let Err(err) = run_prepared(command, &completion, out, "Hermes completion step").await {
        rollback();
        return Err(
            err.context("The bundle was removed again; any previous Hermes software was restored")
        );
    }
    Ok(())
}

pub async fn install(params: HermesLocalInstallParams) -> Result<LocalInstallOutcome> {
    let json = params.json;
    let out = Reporter {
        json,
        tx: params.progress_tx.clone(),
    };
    let platform = LocalPlatform::current()?;
    let paths = LocalPaths::resolve(platform)?;
    let pins = Pins::load()?;
    let settings = &params.settings;
    if params.computer_use && !params.from_source {
        bail!(
            "--computer-use needs --from-source: the pre-built bundle is built without \
             upstream's computer-use driver."
        );
    }
    if params.from_source && params.bundle_archive.is_some() {
        bail!("--bundle-archive installs a pre-built bundle and cannot be combined with --from-source.");
    }

    out.say(&format!(
        "\n[Step 1/{TOTAL_STEPS}] Checking this computer..."
    ));
    let ai_provider = resolve_ai_provider(settings)?;
    let (config_content, ai_summary) = build_config_content(settings)?;
    let env_content = build_env_content(settings)?;
    let telegram_token = settings
        .telegram_bot_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(token) = telegram_token {
        validate_telegram_bot_token(token)?;
    }
    let opencode = match (platform, ai_provider) {
        _ if !params.install_opencode => Err("disabled by --no-opencode"),
        (LocalPlatform::Windows, _) => {
            Err("not supported on Windows yet (upstream installer is bash-only)")
        }
        (_, AiProvider::Bedrock) => Err("needs an OpenRouter API key"),
        _ => settings
            .openrouter_api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .map(|key| OpenCodePlan {
                api_key: key.trim().to_string(),
                model: normalized_openrouter_model(settings),
            })
            .ok_or("needs an OpenRouter API key"),
    };
    let record = load_record(&paths.record)?;
    let findings = existing_install_findings(
        &paths,
        record.as_ref(),
        hermes_on_path(platform),
        find_gateway_service(platform, &paths.home).await,
    );
    let installer_url = pins.installer_url(platform);
    let installer_path = paths.state_dir.join(platform.installer_file());
    let installer = installer_command(
        platform,
        &installer_path,
        &pins.hermes_commit,
        params.computer_use,
    );

    out.say(&format!("  Platform:  {}", platform.label()));
    out.say(&format!(
        "  Hermes:    {} (commit {})",
        pins.hermes_version,
        &pins.hermes_commit[..8]
    ));
    out.say(&format!("  Home:      {}", paths.hermes_home.display()));
    let bundle_source = match &params.bundle_archive {
        Some(archive) => archive.display().to_string(),
        None => pins.bundle_url(platform),
    };
    if params.from_source {
        out.say("  Install:   from source with upstream's installer (compiles on this computer)");
    } else {
        out.say(&format!(
            "  Install:   pre-built bundle {} (nothing is compiled here)",
            pins.release
        ));
    }
    out.say(&format!("  AI model:  {ai_summary}"));
    out.say(&format!(
        "  Terminal:  {}",
        settings.terminal_backend.as_str()
    ));
    if settings.terminal_backend == TerminalBackend::Local {
        out.say(&format!("  {LOCAL_TERMINAL_WARNING}"));
    }

    if params.dry_run {
        if !findings.is_empty() {
            out.say("  Existing install detected; a real run stops here unless --force:");
            for finding in &findings {
                out.say(&format!("    - {finding}"));
            }
        }
        let mut writes = vec![
            paths.hermes_home.join(".env"),
            paths.hermes_home.join("config.yaml"),
        ];
        if opencode.is_ok() {
            writes.push(paths.opencode_config_dir().join("opencode.json"));
            writes.push(paths.opencode_config_dir().join("openrouter-api-key"));
        }
        let gateway = params
            .install_service
            .then(|| "hermes gateway install --start-now --start-on-login".to_string());
        out.say("\n[Dry-run] No changes will be made. Planned actions:");
        if params.from_source {
            out.say(&format!(
                "  Download:  {installer_url} (SHA-256 {})",
                pins.installer_sha256(platform)
            ));
            out.say(&format!("  Run:       {}", installer.display()));
        } else {
            out.say(&format!(
                "  Download:  {bundle_source} (checked against its manifest's SHA-256)"
            ));
            out.say(&format!(
                "  Extract:   {} into {}",
                bundle::ROOTS.join(", "),
                paths.hermes_home.display()
            ));
            out.say("  Run:       upstream's completion step to publish the `hermes` command");
        }
        for path in &writes {
            out.say(&format!("  Write:     {}", path.display()));
        }
        match &opencode {
            Ok(_) => out.say(&format!(
                "  OpenCode:  install {} via {OPENCODE_INSTALLER_URL}",
                pins.opencode_version
            )),
            Err(reason) => out.say(&format!("  OpenCode:  skipped ({reason})")),
        }
        out.say(&format!(
            "  Service:   {}",
            gateway.as_deref().unwrap_or("skipped (--no-service)")
        ));
        if json {
            let payload = serde_json::json!({
                "dry_run": true,
                "platform": platform,
                "hermes_home": paths.hermes_home,
                "hermes_version": pins.hermes_version,
                "hermes_commit": pins.hermes_commit,
                "ai_model": ai_summary,
                "terminal_backend": settings.terminal_backend,
                "existing_install": findings,
                "install_method": if params.from_source { InstallMethod::Source } else { InstallMethod::Bundle },
                "bundle": (!params.from_source).then_some(&bundle_source),
                "installer_url": params.from_source.then_some(&installer_url),
                "installer_command": params.from_source.then(|| installer.display()),
                "writes": writes,
                "opencode": opencode.as_ref().map(|_| pins.opencode_version.clone()).ok(),
                "gateway_service": gateway,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        return Ok(LocalInstallOutcome {
            record: None,
            telegram_bot: None,
            opencode: None,
        });
    }

    if !findings.is_empty() && !params.force {
        let list = findings
            .iter()
            .map(|finding| format!("  - {finding}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "Hermes already appears to be installed on this computer:\n{list}\n\
             To avoid overwriting it, nothing was changed. Remove it first \
             (`clawmacdo hermes-local-uninstall` for installs clawmacdo made, otherwise `hermes uninstall`), \
             or pass --force to reinstall the pinned version and replace the configuration \
             (existing files are backed up first)."
        );
    }
    check_prerequisites(
        platform,
        settings.terminal_backend,
        &paths.home,
        params.from_source,
    )
    .await?;
    let mut telegram_bot = None;
    if let Some(token) = telegram_token {
        telegram_bot = Some(match verify_telegram_bot_token(token).await? {
            TelegramVerification::Verified(username) => {
                out.say(&format!("  Telegram:  verified @{username}"));
                format!("@{username}")
            }
            TelegramVerification::Unverified(reason) => {
                out.say(&format!(
                    "  Telegram:  token accepted; getMe unavailable ({reason})"
                ));
                "configured".to_string()
            }
        });
    }

    let backup_dir = paths
        .state_dir
        .join(format!("backup-{}", Utc::now().format("%Y%m%d-%H%M%S")));
    let software_backup = backup_dir.join("software");
    if params.from_source {
        out.say(&format!(
            "\n[Step 2/{TOTAL_STEPS}] Downloading the pinned Hermes installer..."
        ));
        download_verified(
            &installer_url,
            pins.installer_sha256(platform),
            &installer_path,
        )
        .await?;
        out.say("  Checksum verified.");

        out.say(&format!(
            "\n[Step 3/{TOTAL_STEPS}] Installing Hermes (this can take several minutes)..."
        ));
        let install_result = run_command(&installer, &out, "Hermes installer").await;
        let _ = std::fs::remove_file(&installer_path);
        install_result?;
    } else {
        install_from_bundle(&params, platform, &paths, &pins, &software_backup, &out).await?;
    }
    let launcher = find_launcher(platform, &paths).with_context(|| {
        format!(
            "Hermes finished installing but no `hermes` command was found in: {}",
            launcher_candidates(platform, &paths.home, &paths.hermes_home)
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    out.say(&format!(
        "\n[Step 4/{TOTAL_STEPS}] Writing Hermes configuration..."
    ));
    let mut written_files = Vec::new();
    for (name, contents) in [(".env", &env_content), ("config.yaml", &config_content)] {
        let path = paths.hermes_home.join(name);
        let backup = backup_existing(&path, &backup_dir, &format!("hermes{name}"))?;
        write_private(&path, contents)?;
        out.say(&format!("  Wrote {}", path.display()));
        written_files.push(WrittenFile { path, backup });
    }

    out.say(&format!(
        "\n[Step 5/{TOTAL_STEPS}] Setting up the OpenCode CLI..."
    ));
    let mut opencode_binary_installed = false;
    let mut opencode_summary = None;
    match &opencode {
        Ok(plan) => {
            let opencode_bin = paths.opencode_dir().join("bin").join("opencode");
            if opencode_bin.is_file() {
                out.say("  Using the existing OpenCode CLI.");
            } else {
                let script = paths.state_dir.join("opencode-install.sh");
                download_verified(
                    OPENCODE_INSTALLER_URL,
                    &pins.opencode_installer_sha256,
                    &script,
                )
                .await?;
                let spec = CommandSpec {
                    program: "bash".into(),
                    args: vec![
                        script.display().to_string(),
                        "--version".into(),
                        pins.opencode_version.clone(),
                        "--no-modify-path".into(),
                    ],
                };
                let result = run_command(&spec, &out, "OpenCode installer").await;
                let _ = std::fs::remove_file(&script);
                result?;
                opencode_binary_installed = true;
            }
            let config_dir = paths.opencode_config_dir();
            for (name, contents) in [
                ("opencode.json", build_opencode_config_content(&plan.model)),
                ("openrouter-api-key", plan.api_key.clone()),
            ] {
                let path = config_dir.join(name);
                let backup = backup_existing(&path, &backup_dir, &format!("opencode-{name}"))?;
                write_private(&path, &contents)?;
                written_files.push(WrittenFile { path, backup });
            }
            opencode_summary = Some(opencode_bin.display().to_string());
        }
        Err(reason) => out.say(&format!("  Skipped: {reason}.")),
    }

    out.say(&format!(
        "\n[Step 6/{TOTAL_STEPS}] Registering the Hermes gateway service..."
    ));
    if params.install_service {
        run_command(
            &hermes_command(
                &launcher,
                &["gateway", "install", "--start-now", "--start-on-login"],
            ),
            &out,
            "Hermes gateway service install",
        )
        .await?;
        if let Err(err) = run_command(
            &hermes_command(&launcher, &["gateway", "status"]),
            &out,
            "Hermes gateway status",
        )
        .await
        {
            out.say(&format!(
                "  WARNING: {err}. Check it later with `hermes gateway status`."
            ));
        }
    } else {
        out.say("  Skipped (--no-service). Start it manually with `hermes gateway run`.");
    }

    let backup_dir = backup_dir.exists().then_some(backup_dir);
    let record = LocalInstallRecord {
        platform,
        hermes_home: paths.hermes_home.clone(),
        launcher: launcher.clone(),
        hermes_version: pins.hermes_version.clone(),
        hermes_commit: pins.hermes_commit.clone(),
        ai_model: ai_summary.clone(),
        terminal_backend: settings.terminal_backend,
        service_installed: params.install_service,
        opencode_binary_installed,
        written_files,
        backup_dir,
        installed_at: Utc::now(),
        install_method: if params.from_source {
            InstallMethod::Source
        } else {
            InstallMethod::Bundle
        },
        bundle_release: (!params.from_source).then(|| pins.release.clone()),
    };
    write_private(&paths.record, &serde_json::to_string_pretty(&record)?)?;

    if out.tx.is_some() {
        // The web UI renders its own summary from the returned outcome.
    } else if json {
        let mut payload = serde_json::to_value(&record)?;
        payload["record_path"] = serde_json::json!(paths.record);
        payload["telegram_bot"] = serde_json::json!(telegram_bot);
        payload["opencode"] = serde_json::json!(opencode_summary);
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_install_summary(
            &record,
            telegram_bot.as_deref(),
            opencode_summary.as_deref(),
        );
    }
    Ok(LocalInstallOutcome {
        record: Some(record),
        telegram_bot,
        opencode: opencode_summary,
    })
}

pub fn install_source_label(record: &LocalInstallRecord) -> String {
    match record.install_method {
        InstallMethod::Bundle => format!(
            "pre-built bundle {}",
            record
                .bundle_release
                .as_deref()
                .unwrap_or("(unknown release)")
        ),
        InstallMethod::Source => "source (upstream installer)".to_string(),
    }
}

fn print_install_summary(
    record: &LocalInstallRecord,
    telegram_bot: Option<&str>,
    opencode: Option<&str>,
) {
    let divider = "=".repeat(60);
    println!("\n{divider}");
    println!("  Hermes Agent installed on this computer");
    println!("{divider}");
    println!("  Platform:          {}", record.platform.label());
    println!(
        "  Hermes:            {} (commit {})",
        record.hermes_version,
        &record.hermes_commit[..8]
    );
    println!("  Installed from:    {}", install_source_label(record));
    println!("  Command:           {}", record.launcher.display());
    println!("  Hermes home:       {}", record.hermes_home.display());
    println!("  AI model:          {}", record.ai_model);
    println!("  Terminal backend:  {}", record.terminal_backend.as_str());
    if record.service_installed {
        println!("  Gateway service:   installed; starts at login");
    } else {
        println!("  Gateway service:   not installed; run `hermes gateway run`");
    }
    if let Some(bot) = telegram_bot {
        println!("  Telegram bot:      {bot}");
    }
    match opencode {
        Some(path) => println!("  OpenCode CLI:      {path}"),
        None => println!("  OpenCode CLI:      not installed"),
    }
    if let Some(dir) = &record.backup_dir {
        println!("  Previous config:   backed up to {}", dir.display());
        if dir.join("software").is_dir() {
            println!(
                "  Previous software: moved to {} (delete it once Hermes works)",
                dir.join("software").display()
            );
        }
    }
    println!();
    println!("  Chat:              hermes");
    println!("  Dashboard:         hermes dashboard   (local only, on demand)");
    println!("  Uninstall:         clawmacdo hermes-local-uninstall");
    println!();
    println!("  Open a new terminal so `hermes` is on your PATH.");
    println!("  The gateway and Telegram bot only respond while this computer is awake.");
    if record.terminal_backend == TerminalBackend::Local {
        println!("  {LOCAL_TERMINAL_WARNING}");
    }
    println!("{divider}");
}

pub struct HermesLocalUninstallParams {
    pub keep_data: bool,
    pub yes: bool,
    pub force: bool,
    pub dry_run: bool,
    pub json: bool,
    /// Web UI progress channel; the web UI confirms first and passes `yes`.
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
}

/// What an uninstall did; `performed` is false for dry runs and cancelled prompts.
#[cfg_attr(not(feature = "web-ui"), allow(dead_code))]
pub struct LocalUninstallOutcome {
    pub performed: bool,
    pub data_removed: bool,
    pub warnings: Vec<String>,
}

/// What uninstall will do for one recorded file.
#[derive(Debug, PartialEq, Eq)]
enum FileAction {
    Restore { path: PathBuf, backup: PathBuf },
    Remove(PathBuf),
    Keep(PathBuf),
}

/// Hermes files go with the data folder; OpenCode files are restored or removed.
fn file_actions(record: &LocalInstallRecord, keep_data: bool) -> Vec<FileAction> {
    record
        .written_files
        .iter()
        .filter(|file| !file.path.starts_with(&record.hermes_home))
        .map(|file| match (&file.backup, keep_data) {
            (Some(backup), _) => FileAction::Restore {
                path: file.path.clone(),
                backup: backup.clone(),
            },
            (None, true) => FileAction::Keep(file.path.clone()),
            (None, false) => FileAction::Remove(file.path.clone()),
        })
        .collect()
}

pub async fn uninstall(params: HermesLocalUninstallParams) -> Result<LocalUninstallOutcome> {
    let json = params.json;
    let out = Reporter {
        json,
        tx: params.progress_tx.clone(),
    };
    let not_performed = LocalUninstallOutcome {
        performed: false,
        data_removed: false,
        warnings: Vec::new(),
    };
    let platform = LocalPlatform::current()?;
    let paths = LocalPaths::resolve(platform)?;
    let record = load_record(&paths.record)?;
    if record.is_none() && !params.force {
        bail!(
            "No Hermes install made by clawmacdo was found ({} is missing). \
             clawmacdo only removes installs it created. To remove another Hermes install, \
             run `hermes uninstall`, or pass --force to run it through clawmacdo anyway.",
            paths.record.display()
        );
    }
    let hermes_home = record
        .as_ref()
        .map(|record| record.hermes_home.clone())
        .unwrap_or_else(|| paths.hermes_home.clone());
    let launcher = record
        .as_ref()
        .map(|record| record.launcher.clone())
        .filter(|path| path.is_file())
        .or_else(|| find_launcher(platform, &paths));
    if record.is_none() && launcher.is_none() {
        bail!(
            "Nothing to uninstall: no clawmacdo install record and no `hermes` command under {}.",
            hermes_home.display()
        );
    }
    let hermes_args = uninstall_args(params.keep_data);
    let actions = record
        .as_ref()
        .map(|record| file_actions(record, params.keep_data))
        .unwrap_or_default();
    // The OpenCode binary is code, so it goes even with --keep-data (like Hermes's own code).
    let remove_opencode = record
        .as_ref()
        .is_some_and(|record| record.opencode_binary_installed);
    let backup_dir = record.as_ref().and_then(|record| record.backup_dir.clone());

    out.say("Hermes local uninstall plan:");
    match &launcher {
        Some(launcher) => out.say(&format!(
            "  Run:       {}  (stops the gateway service, removes code and PATH entries{})",
            hermes_command(launcher, &hermes_args).display(),
            if params.keep_data {
                ""
            } else {
                ", deletes the data folder"
            }
        )),
        None => out.say(
            "  Hermes command not found; only the files clawmacdo recorded will be cleaned up.",
        ),
    }
    if params.keep_data {
        out.say(&format!(
            "  Keep:      {} (configuration, .env API keys, sessions, memory)",
            hermes_home.display()
        ));
    } else {
        out.say(&format!(
            "  Delete:    {} (configuration, .env API keys, sessions, memory)",
            hermes_home.display()
        ));
    }
    for action in &actions {
        match action {
            FileAction::Restore { path, backup } => out.say(&format!(
                "  Restore:   {} from {}",
                path.display(),
                backup.display()
            )),
            FileAction::Remove(path) => out.say(&format!("  Delete:    {}", path.display())),
            FileAction::Keep(path) => out.say(&format!("  Keep:      {}", path.display())),
        }
    }
    if remove_opencode {
        out.say(&format!(
            "  Delete:    {} (OpenCode CLI installed by clawmacdo)",
            paths.opencode_dir().display()
        ));
    }
    if let Some(dir) = backup_dir.as_ref().filter(|_| !params.keep_data) {
        out.say(&format!(
            "  Delete:    {} (config backups, may contain old API keys)",
            dir.display()
        ));
    }

    if params.dry_run {
        out.say("\n[Dry-run] Nothing was changed.");
        if json {
            let payload = serde_json::json!({
                "dry_run": true,
                "record_found": record.is_some(),
                "hermes_home": hermes_home,
                "keep_data": params.keep_data,
                "hermes_command": launcher.as_ref().map(|launcher| hermes_command(launcher, &hermes_args).display()),
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        return Ok(not_performed);
    }
    if !params.yes {
        if json {
            bail!("--json needs --yes because uninstall cannot prompt for confirmation.");
        }
        let prompt = if params.keep_data {
            "Remove Hermes from this computer (keeping its data folder)?"
        } else {
            "Remove Hermes and permanently delete its data folder, API keys, sessions and memory?"
        };
        if !Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact()?
        {
            println!("Cancelled.");
            return Ok(not_performed);
        }
    }

    let mut warnings = Vec::new();
    match &launcher {
        Some(launcher) => {
            let spec = hermes_command(launcher, &hermes_args);
            let mut command = spec.command();
            command.env("HERMES_HOME", &hermes_home);
            run_prepared(command, &spec, &out, "`hermes uninstall`")
                .await
                .context("The clawmacdo install record was kept so you can retry")?;
            let target = uninstall_target(&hermes_home, params.keep_data);
            if target.exists() {
                if !safe_to_delete(&target, &paths.home) {
                    warnings.push(format!(
                        "Refusing to delete {} because it contains your home folder.",
                        target.display()
                    ));
                } else if let Err(err) = remove_leftovers(&target).await {
                    warnings.push(format!(
                        "`hermes uninstall` left {} behind and it could not be deleted ({err}). \
                         Delete it manually{}.",
                        target.display(),
                        if params.keep_data {
                            ""
                        } else {
                            "; it may contain API keys"
                        }
                    ));
                }
            }
        }
        None => warnings.push(format!(
            "Hermes command not found; remove {} manually if it is still present.",
            hermes_home.display()
        )),
    }
    for action in &actions {
        match action {
            FileAction::Restore { path, backup } => {
                std::fs::copy(backup, path)
                    .with_context(|| format!("Failed to restore {}", path.display()))?;
            }
            FileAction::Remove(path) => match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => warnings.push(format!("Could not delete {}: {err}", path.display())),
            },
            FileAction::Keep(_) => {}
        }
    }
    if remove_opencode {
        if let Err(err) = std::fs::remove_dir_all(paths.opencode_dir()) {
            if err.kind() != std::io::ErrorKind::NotFound {
                warnings.push(format!(
                    "Could not delete {}: {err}",
                    paths.opencode_dir().display()
                ));
            }
        }
    }
    if let Some(dir) = backup_dir.as_ref().filter(|_| !params.keep_data) {
        let _ = std::fs::remove_dir_all(dir);
    }
    if record.is_some() {
        std::fs::remove_file(&paths.record)
            .with_context(|| format!("Failed to delete {}", paths.record.display()))?;
    }
    let _ = std::fs::remove_dir(&paths.state_dir);

    for warning in &warnings {
        out.say(&format!("  WARNING: {warning}"));
    }
    let data_removed = !params.keep_data && !hermes_home.exists();
    if out.tx.is_some() {
        // The web UI reports the outcome itself.
    } else if json {
        let payload = serde_json::json!({
            "uninstalled": true,
            "hermes_home": hermes_home,
            "data_kept": params.keep_data,
            "data_removed": data_removed,
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if params.keep_data {
        println!(
            "\nHermes was removed. Its data folder was kept: {}",
            hermes_home.display()
        );
    } else if data_removed {
        println!("\nHermes and its data were removed from this computer.");
    } else {
        println!(
            "\nHermes was removed, but its data folder is still present: {}",
            hermes_home.display()
        );
    }
    Ok(LocalUninstallOutcome {
        performed: true,
        data_removed,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("clawmacdo-hermes-local-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_paths(root: &Path) -> LocalPaths {
        LocalPaths {
            home: root.join("home"),
            hermes_home: root.join("home").join(".hermes"),
            state_dir: root.join("state").join(STATE_DIR),
            record: root.join("state").join(RECORD_FILE),
        }
    }

    fn test_record(paths: &LocalPaths, written_files: Vec<WrittenFile>) -> LocalInstallRecord {
        LocalInstallRecord {
            platform: LocalPlatform::MacOs,
            hermes_home: paths.hermes_home.clone(),
            launcher: paths.home.join(".local/bin/hermes"),
            hermes_version: "0.22.5".into(),
            hermes_commit: "bf0203a794a07aac3ff764972ab9087baa4e4f4e".into(),
            ai_model: "OpenRouter openai/gpt-6-sol".into(),
            terminal_backend: TerminalBackend::Local,
            service_installed: true,
            opencode_binary_installed: true,
            written_files,
            backup_dir: None,
            installed_at: Utc::now(),
            install_method: InstallMethod::Bundle,
            bundle_release: Some("hermes-bundle-v0.22.5-2".into()),
        }
    }

    #[test]
    fn records_from_before_bundles_load_as_source_installs() {
        let dir = TempDir::new();
        let paths = test_paths(&dir.0);
        let mut value = serde_json::to_value(test_record(&paths, Vec::new())).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("install_method");
        object.remove("bundle_release");
        let record: LocalInstallRecord = serde_json::from_value(value).unwrap();
        assert_eq!(record.install_method, InstallMethod::Source);
        assert_eq!(install_source_label(&record), "source (upstream installer)");
        let bundled = test_record(&paths, Vec::new());
        assert_eq!(
            install_source_label(&bundled),
            "pre-built bundle hermes-bundle-v0.22.5-2"
        );
    }

    #[test]
    fn bundle_url_and_completion_follow_the_pins_and_manifest() {
        let pins = Pins::load().unwrap();
        assert_eq!(
            pins.bundle_url(LocalPlatform::MacOs),
            format!(
                "https://github.com/{}/releases/download/{}/hermes-macos-arm64.tar.gz",
                pins.repository, pins.release
            )
        );
        assert!(pins
            .bundle_url(LocalPlatform::Windows)
            .ends_with("/hermes-windows-x64.tar.gz"));

        let manifest = BundleManifest {
            schema: 1,
            release: pins.release.clone(),
            platform: "macos-arm64".into(),
            filename: "hermes-macos-arm64.tar.gz".into(),
            sha256: "a".repeat(64),
            size: 1,
            hermes_commit: pins.hermes_commit.clone(),
            hermes_version: pins.hermes_version.clone(),
            build_home: "/Users/runner".into(),
            build_hermes_home: "/Users/runner/.hermes".into(),
            install_key: "0123456789abcdef".into(),
            store_python: "tools/python-3.14.9/bin/python3".into(),
            relocate: Vec::new(),
        };
        bundle::validate_manifest(&manifest, &pins.expected_bundle(LocalPlatform::MacOs)).unwrap();
        let home = Path::new("/Users/alice/.hermes");
        let spec = completion_command(home, &manifest).unwrap();
        assert_eq!(
            spec.program,
            home.join("tools/python-3.14.9/bin/python3")
                .display()
                .to_string()
        );
        assert_eq!(
            spec.args,
            [
                "-I",
                "-B",
                "-X",
                "utf8",
                "hermes_cli/source_completion.py",
                "--source",
                &home.join("hermes-agent").display().to_string()
            ]
        );
    }

    #[test]
    fn pins_cover_both_installers() {
        let pins = Pins::load().unwrap();
        assert_eq!(pins.hermes_commit.len(), 40);
        assert_eq!(pins.installer_sha256(LocalPlatform::MacOs).len(), 64);
        assert_eq!(pins.installer_sha256(LocalPlatform::Windows).len(), 64);
        assert!(pins
            .installer_url(LocalPlatform::Windows)
            .ends_with(&format!("{}/scripts/install.ps1", pins.hermes_commit)));
    }

    #[test]
    fn macos_installer_is_pinned_and_skips_computer_use_by_default() {
        let spec = installer_command(
            LocalPlatform::MacOs,
            Path::new("/tmp/install.sh"),
            "abc123",
            false,
        );
        assert_eq!(spec.program, "bash");
        assert_eq!(
            spec.args,
            [
                "/tmp/install.sh",
                "--non-interactive",
                "--commit",
                "abc123",
                "--skip-computer-use"
            ]
        );
        let with_computer_use = installer_command(
            LocalPlatform::MacOs,
            Path::new("/tmp/install.sh"),
            "abc123",
            true,
        );
        assert!(!with_computer_use
            .args
            .contains(&"--skip-computer-use".to_string()));
    }

    #[test]
    fn windows_installer_bypasses_policy_only_for_the_pinned_script() {
        let spec = installer_command(
            LocalPlatform::Windows,
            Path::new("C:/state/install.ps1"),
            "abc123",
            false,
        );
        assert_eq!(spec.program, "powershell.exe");
        assert_eq!(
            spec.args,
            [
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                "C:/state/install.ps1",
                "-NonInteractive",
                "-Commit",
                "abc123",
                "-SkipComputerUse"
            ]
        );
    }

    #[test]
    fn hermes_home_follows_upstream_defaults() {
        let home = Path::new("/Users/test");
        assert_eq!(
            hermes_home_for(LocalPlatform::MacOs, None, home, None).unwrap(),
            home.join(".hermes")
        );
        assert_eq!(
            hermes_home_for(
                LocalPlatform::Windows,
                None,
                home,
                Some("C:/Users/test/AppData/Local".into())
            )
            .unwrap(),
            PathBuf::from("C:/Users/test/AppData/Local").join("hermes")
        );
        assert!(hermes_home_for(LocalPlatform::Windows, None, home, None).is_err());
        assert_eq!(
            hermes_home_for(LocalPlatform::Windows, Some("D:/hermes".into()), home, None).unwrap(),
            PathBuf::from("D:/hermes")
        );
    }

    #[test]
    fn windows_cmd_launchers_run_through_cmd() {
        let cmd = hermes_command(
            Path::new("C:/hermes/bin/hermes.cmd"),
            &["gateway", "status"],
        );
        assert_eq!(cmd.program, "cmd");
        assert_eq!(
            cmd.args,
            ["/C", "C:/hermes/bin/hermes.cmd", "gateway", "status"]
        );
        let exe = hermes_command(
            Path::new("C:/hermes/bin/hermes.exe"),
            &["gateway", "status"],
        );
        assert_eq!(exe.program, "C:/hermes/bin/hermes.exe");
        assert_eq!(exe.args, ["gateway", "status"]);
        assert_eq!(
            launcher_candidates(
                LocalPlatform::Windows,
                Path::new("C:/Users/t"),
                Path::new("C:/h")
            ),
            [
                Path::new("C:/h").join("bin").join("hermes.exe"),
                Path::new("C:/h").join("bin").join("hermes.cmd")
            ]
        );
    }

    #[test]
    fn uninstall_deletes_data_unless_kept() {
        assert_eq!(uninstall_args(false), ["uninstall", "--yes", "--full"]);
        assert_eq!(uninstall_args(true), ["uninstall", "--yes"]);
    }

    #[test]
    fn leftover_cleanup_targets_only_what_hermes_uninstall_removes() {
        let hermes_home = Path::new("/Users/test/.hermes");
        assert_eq!(uninstall_target(hermes_home, false), hermes_home);
        assert_eq!(
            uninstall_target(hermes_home, true),
            hermes_home.join("hermes-agent")
        );

        let home = Path::new("/Users/test");
        assert!(safe_to_delete(hermes_home, home));
        assert!(safe_to_delete(Path::new("D:/hermes"), home));
        assert!(!safe_to_delete(home, home));
        assert!(!safe_to_delete(Path::new("/Users"), home));
        assert!(!safe_to_delete(Path::new("/"), home));
    }

    #[tokio::test]
    async fn leftover_cleanup_removes_the_tree_and_tolerates_missing_paths() {
        let dir = TempDir::new();
        let target = dir.0.join("hermes");
        write_private(&target.join("hermes-agent/.git/objects/pack/pack.idx"), "x").unwrap();
        write_private(&target.join(".env"), "OPENROUTER_API_KEY=old\n").unwrap();
        remove_leftovers(&target).await.unwrap();
        assert!(!target.exists());
        remove_leftovers(&target).await.unwrap();
    }

    #[test]
    fn installer_output_is_cleaned_for_the_web_ui() {
        assert_eq!(clean_output_line("\u{1b}[32m✓ done\u{1b}[0m"), "✓ done");
        assert_eq!(
            clean_output_line("10%\r50%\r100% complete  "),
            "100% complete"
        );
        assert_eq!(clean_output_line("plain"), "plain");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn web_progress_streams_child_stdout_and_stderr() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let out = Reporter {
            json: false,
            tx: Some(tx),
        };
        out.say("[Step 1/6] Checking this computer...");
        let spec = CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "echo out; echo err >&2".into()],
        };
        run_command(&spec, &out, "test command").await.unwrap();
        let failing = CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "exit 3".into()],
        };
        let err = run_command(&failing, &out, "failing command")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("failing command failed"));
        drop(out);
        let mut lines = Vec::new();
        while let Ok(line) = rx.try_recv() {
            lines.push(line);
        }
        assert_eq!(lines[0], "[Step 1/6] Checking this computer...");
        assert!(lines.contains(&"    out".to_string()), "{lines:?}");
        assert!(lines.contains(&"    err".to_string()), "{lines:?}");
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let expected = hex::encode(Sha256::digest(b"echo hermes"));
        assert!(verify_sha256(b"echo hermes", &expected).is_ok());
        assert!(verify_sha256(b"echo hermes", &expected.to_uppercase()).is_ok());
        let err = verify_sha256(b"echo tampered", &expected).unwrap_err();
        assert!(err.to_string().contains("Checksum mismatch"));
    }

    #[test]
    fn existing_install_is_detected_from_config_path_record_or_service() {
        let dir = TempDir::new();
        let paths = test_paths(&dir.0);
        assert!(existing_install_findings(&paths, None, None, None).is_empty());

        write_private(&paths.hermes_home.join("config.yaml"), "model: {}\n").unwrap();
        let record = test_record(&paths, Vec::new());
        let findings = existing_install_findings(
            &paths,
            Some(&record),
            Some(PathBuf::from("/opt/homebrew/bin/hermes")),
            Some("ai.hermes.gateway.plist".into()),
        );
        assert_eq!(findings.len(), 4, "{findings:?}");
        assert!(findings[0].contains("clawmacdo installed Hermes here"));
        assert!(findings.iter().any(|f| f.contains("config.yaml")));
        assert!(findings
            .iter()
            .any(|f| f.contains("/opt/homebrew/bin/hermes")));
        assert!(findings.iter().any(|f| f.contains("ai.hermes.gateway")));
    }

    #[cfg(unix)]
    #[test]
    fn npm_cache_dir_prefers_the_configured_cache() {
        let home = Path::new("/Users/test");
        assert_eq!(npm_cache_dir(home, None), home.join(".npm"));
        assert_eq!(npm_cache_dir(home, Some("".into())), home.join(".npm"));
        assert_eq!(
            npm_cache_dir(home, Some("/var/cache/npm".into())),
            PathBuf::from("/var/cache/npm")
        );
    }

    #[cfg(unix)]
    #[test]
    fn npm_cache_ownership_check_reports_entries_owned_by_another_user() {
        use std::os::unix::fs::MetadataExt;
        let dir = TempDir::new();
        let cache = dir.0.join("_cacache");
        std::fs::create_dir_all(cache.join("content-v2/sha512/78/00")).unwrap();
        std::fs::write(cache.join("content-v2/sha512/78/00/blob"), "x").unwrap();
        let uid = std::fs::metadata(&dir.0).unwrap().uid();

        assert_eq!(first_foreign_owned(&cache, uid), None);
        assert_eq!(first_foreign_owned(&cache, uid + 1), Some(cache.clone()));
        assert_eq!(first_foreign_owned(&dir.0.join("missing"), uid + 1), None);
    }

    #[test]
    fn overwritten_files_are_backed_up_privately() {
        let dir = TempDir::new();
        let original = dir.0.join(".env");
        let backups = dir.0.join("backup");
        assert_eq!(
            backup_existing(&original, &backups, "hermes.env").unwrap(),
            None
        );
        assert!(!backups.exists());

        std::fs::write(&original, "OPENROUTER_API_KEY=old\n").unwrap();
        let saved = backup_existing(&original, &backups, "hermes.env")
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&saved).unwrap(),
            "OPENROUTER_API_KEY=old\n"
        );
        write_private(&original, "OPENROUTER_API_KEY=new\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&saved).unwrap(),
            "OPENROUTER_API_KEY=old\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&original, &saved] {
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }

    #[test]
    fn install_record_round_trips() {
        let dir = TempDir::new();
        let paths = test_paths(&dir.0);
        let record = test_record(
            &paths,
            vec![WrittenFile {
                path: paths.hermes_home.join(".env"),
                backup: Some(dir.0.join("backup/hermes.env")),
            }],
        );
        write_private(
            &paths.record,
            &serde_json::to_string_pretty(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(load_record(&paths.record).unwrap(), Some(record));
        std::fs::write(&paths.record, "{not json").unwrap();
        assert!(load_record(&paths.record).is_err());
        assert_eq!(load_record(&dir.0.join("missing.json")).unwrap(), None);
    }

    #[test]
    fn uninstall_restores_backups_and_leaves_hermes_files_to_hermes() {
        let dir = TempDir::new();
        let paths = test_paths(&dir.0);
        let opencode_json = paths.opencode_config_dir().join("opencode.json");
        let key_file = paths.opencode_config_dir().join("openrouter-api-key");
        let backup = dir.0.join("backup/opencode-opencode.json");
        let record = test_record(
            &paths,
            vec![
                WrittenFile {
                    path: paths.hermes_home.join(".env"),
                    backup: None,
                },
                WrittenFile {
                    path: opencode_json.clone(),
                    backup: Some(backup.clone()),
                },
                WrittenFile {
                    path: key_file.clone(),
                    backup: None,
                },
            ],
        );
        assert_eq!(
            file_actions(&record, false),
            [
                FileAction::Restore {
                    path: opencode_json.clone(),
                    backup: backup.clone()
                },
                FileAction::Remove(key_file.clone()),
            ]
        );
        assert_eq!(
            file_actions(&record, true),
            [
                FileAction::Restore {
                    path: opencode_json,
                    backup
                },
                FileAction::Keep(key_file)
            ]
        );
    }
}
