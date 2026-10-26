//! The pre-built local Hermes bundle: download, verify, extract and relocate.
//!
//! CI builds Hermes on clean macOS and Windows runners with upstream's installer, so
//! nothing compiles on this computer. The software is built in the runner's Hermes home.
//! Installing it here renames upstream's per-install state folder (named from the
//! checkout path) and rewrites the text files the build listed as naming its old
//! location; upstream's completion step then publishes launchers for this location.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// Folders under the Hermes home that the bundle provides.
pub(super) const ROOTS: [&str; 3] = ["hermes-agent", "tools", "installs"];
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 600_000;
const DOWNLOAD_ATTEMPTS: u32 = 4;
const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const SMALL_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Written by `scripts/hermes-bundle/package.py` next to each local archive.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(super) struct BundleManifest {
    pub schema: u32,
    pub release: String,
    pub platform: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
    pub hermes_commit: String,
    pub hermes_version: String,
    pub build_home: String,
    pub build_hermes_home: String,
    pub install_key: String,
    pub store_python: String,
    #[serde(default)]
    pub relocate: Vec<String>,
}

/// The release, platform and Hermes pins this binary accepts.
pub(super) struct ExpectedBundle<'a> {
    pub release: &'a str,
    pub platform: &'a str,
    pub hermes_commit: &'a str,
    pub hermes_version: &'a str,
}

impl ExpectedBundle<'_> {
    pub(super) fn filename(&self) -> String {
        format!("hermes-{}.tar.gz", self.platform)
    }
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn validate_manifest(
    manifest: &BundleManifest,
    expected: &ExpectedBundle,
) -> Result<()> {
    if manifest.schema != 1
        || manifest.release != expected.release
        || manifest.platform != expected.platform
        || manifest.filename != expected.filename()
        || manifest.hermes_commit != expected.hermes_commit
        || manifest.hermes_version != expected.hermes_version
    {
        bail!(
            "The Hermes bundle manifest does not match this clawmacdo version \
             (expected {} {} for Hermes {}, got {} {} for Hermes {}).",
            expected.release,
            expected.platform,
            expected.hermes_version,
            manifest.release,
            manifest.platform,
            manifest.hermes_version
        );
    }
    if !is_hex(&manifest.sha256, 64) {
        bail!("The Hermes bundle manifest has an invalid SHA-256.");
    }
    if manifest.size == 0 || manifest.size > MAX_ARCHIVE_BYTES {
        bail!(
            "The Hermes bundle manifest has an invalid size ({} bytes).",
            manifest.size
        );
    }
    if !is_hex(&manifest.install_key, 16)
        || manifest.build_home.is_empty()
        || manifest.build_hermes_home.is_empty()
    {
        bail!("The Hermes bundle manifest does not describe its build location.");
    }
    let store_python = software_path(&manifest.store_python)?;
    if !store_python.starts_with("tools") {
        bail!(
            "The bundle's Python must be in its tool store, not {}",
            manifest.store_python
        );
    }
    for path in &manifest.relocate {
        // Upstream verifies the tool store against recorded digests; rewriting it breaks them.
        if software_path(path)?.starts_with("tools") {
            bail!("The Hermes bundle manifest asks to rewrite the tool store ({path})");
        }
    }
    Ok(())
}

/// A relative path inside one of the bundle's software folders.
pub(super) fn software_path(name: &str) -> Result<PathBuf> {
    let mut path = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => path.push(part),
            _ => bail!("Unexpected path in the Hermes bundle: {name}"),
        }
    }
    let first = path.components().next().map(|c| c.as_os_str().to_owned());
    if !first.is_some_and(|first| ROOTS.iter().any(|root| first == *root)) {
        bail!("Path outside the Hermes software folders in the bundle: {name}");
    }
    Ok(path)
}

/// A symlink stays relative and resolves inside the software folders.
fn check_symlink(entry: &Path, target: &Path) -> Result<()> {
    let mut resolved: Vec<std::ffi::OsString> = entry
        .parent()
        .map(|parent| parent.iter().map(ToOwned::to_owned).collect())
        .unwrap_or_default();
    for component in target.components() {
        match component {
            Component::Normal(part) => resolved.push(part.to_owned()),
            Component::CurDir => {}
            Component::ParentDir if resolved.pop().is_some() => {}
            _ => bail!(
                "Bundle link escapes the software folders: {} -> {}",
                entry.display(),
                target.display()
            ),
        }
    }
    let resolved: PathBuf = resolved.iter().collect();
    software_path(&resolved.to_string_lossy())
        .with_context(|| format!("Bundle link {} -> {}", entry.display(), target.display()))?;
    Ok(())
}

/// Why a missing bundle stops the install instead of compiling from source.
pub(super) fn unavailable_message(url: &str, platform: &str) -> String {
    format!(
        "The pre-built Hermes bundle for {platform} is not published ({url}). \
         Nothing was changed. Re-run with --from-source to build Hermes from source \
         on this computer instead (slower; compiles the dashboard and terminal UI)."
    )
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("clawmacdo/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create an HTTP client")
}

/// How long to wait before retrying after failed attempt `attempt` (1-based), or `None`
/// when another attempt cannot help. GitHub answers busy shared addresses (CI runners,
/// offices) with 429; `status` is `None` for network errors and timeouts.
pub(super) fn retry_delay(
    status: Option<reqwest::StatusCode>,
    retry_after: Option<&str>,
    attempt: u32,
) -> Option<std::time::Duration> {
    if attempt >= DOWNLOAD_ATTEMPTS {
        return None;
    }
    if status.is_some_and(|status| {
        status != reqwest::StatusCode::TOO_MANY_REQUESTS && !status.is_server_error()
    }) {
        return None;
    }
    let backoff = 5 * u64::from(attempt * attempt);
    let seconds = retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(backoff);
    Some(std::time::Duration::from_secs(seconds.min(60)))
}

/// Download a small file into memory, retrying rate limits, server errors and network
/// failures. `Ok(None)` means the server answered 404.
pub(super) async fn fetch_bytes(
    url: &str,
    say: &(dyn Fn(&str) + Send + Sync),
) -> Result<Option<Vec<u8>>> {
    let client = http_client()?;
    let mut attempt = 1;
    loop {
        let request = async {
            let response = client.get(url).send().await?;
            let status = response.status();
            if !status.is_success() {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                return Ok(Err((status, retry_after)));
            }
            response.bytes().await.map(|bytes| Ok(bytes.to_vec()))
        };
        let (status, retry_after, err) =
            match tokio::time::timeout(SMALL_DOWNLOAD_TIMEOUT, request).await {
                Ok(Ok(Ok(bytes))) => return Ok(Some(bytes)),
                Ok(Ok(Err((status, _)))) if status == reqwest::StatusCode::NOT_FOUND => {
                    return Ok(None)
                }
                Ok(Ok(Err((status, retry_after)))) => (
                    Some(status),
                    retry_after,
                    anyhow::anyhow!("Failed to download {url}: HTTP {status}"),
                ),
                Ok(Err(err)) => (
                    None,
                    None,
                    anyhow::Error::new(err).context(format!("Failed to download {url}")),
                ),
                Err(_) => (
                    None,
                    None,
                    anyhow::anyhow!("Failed to download {url}: timed out"),
                ),
            };
        let Some(delay) = retry_delay(status, retry_after.as_deref(), attempt) else {
            return Err(err);
        };
        say(&format!(
            "  {err:#}; retrying in {}s ({}/{DOWNLOAD_ATTEMPTS})...",
            delay.as_secs(),
            attempt + 1
        ));
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

pub(super) async fn fetch_manifest(
    url: &str,
    platform: &str,
    say: &(dyn Fn(&str) + Send + Sync),
) -> Result<BundleManifest> {
    let Some(bytes) = fetch_bytes(url, say).await? else {
        bail!("{}", unavailable_message(url, platform));
    };
    if bytes.len() > MAX_MANIFEST_BYTES {
        bail!("The Hermes bundle manifest at {url} is too large.");
    }
    serde_json::from_slice(&bytes)
        .with_context(|| format!("Invalid Hermes bundle manifest at {url}"))
}

/// Read the manifest saved next to a local archive (CI verification).
pub(super) fn read_local_manifest(archive: &Path) -> Result<BundleManifest> {
    let mut name = archive.as_os_str().to_owned();
    name.push(".json");
    let path = PathBuf::from(name);
    let contents = std::fs::read(&path)
        .with_context(|| format!("Failed to read the bundle manifest {}", path.display()))?;
    serde_json::from_slice(&contents)
        .with_context(|| format!("Invalid Hermes bundle manifest {}", path.display()))
}

enum DownloadError {
    Retry(anyhow::Error),
    Fatal(anyhow::Error),
}

async fn download_once(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    size: u64,
    not_found: &str,
    say: &(dyn Fn(&str) + Send + Sync),
) -> std::result::Result<(), DownloadError> {
    let retry = |err: anyhow::Error| DownloadError::Retry(err);
    let mut response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to download {url}"))
        .map_err(retry)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(DownloadError::Fatal(anyhow::anyhow!("{not_found}")));
    }
    if response.url().scheme() != "https" {
        return Err(DownloadError::Fatal(anyhow::anyhow!(
            "The download of {url} was redirected away from HTTPS."
        )));
    }
    if let Err(err) = response.error_for_status_ref() {
        let fatal = err.status().is_some_and(|status| {
            status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS
        });
        let err = anyhow::Error::new(err).context(format!("Failed to download {url}"));
        return Err(if fatal {
            DownloadError::Fatal(err)
        } else {
            DownloadError::Retry(err)
        });
    }
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("Failed to create {}", dest.display()))
        .map_err(DownloadError::Fatal)?;
    let mut received: u64 = 0;
    let mut next_report = 10;
    loop {
        let chunk = tokio::time::timeout(CHUNK_TIMEOUT, response.chunk())
            .await
            .map_err(|_| retry(anyhow::anyhow!("The download of {url} stalled")))?
            .with_context(|| format!("Failed to read {url}"))
            .map_err(retry)?;
        let Some(chunk) = chunk else { break };
        received += chunk.len() as u64;
        if received > size {
            return Err(DownloadError::Fatal(anyhow::anyhow!(
                "The download of {url} is larger than expected ({size} bytes)."
            )));
        }
        file.write_all(&chunk)
            .await
            .with_context(|| format!("Failed to write {}", dest.display()))
            .map_err(DownloadError::Fatal)?;
        let percent = received * 100 / size;
        if percent >= next_report {
            say(&format!(
                "  Downloaded {} of {} MiB ({percent}%)",
                received / (1024 * 1024),
                size / (1024 * 1024)
            ));
            next_report = (percent / 10 + 1) * 10;
        }
    }
    file.flush()
        .await
        .with_context(|| format!("Failed to write {}", dest.display()))
        .map_err(DownloadError::Fatal)?;
    if received != size {
        return Err(retry(anyhow::anyhow!(
            "The download of {url} ended early ({received} of {size} bytes)."
        )));
    }
    Ok(())
}

/// Stream the bundle archive to `dest`, retrying interrupted downloads.
pub(super) async fn download_archive(
    url: &str,
    dest: &Path,
    manifest: &BundleManifest,
    say: &(dyn Fn(&str) + Send + Sync),
) -> Result<()> {
    let not_found = unavailable_message(url, &manifest.platform);
    download_file(url, dest, manifest.size, &not_found, say).await
}

/// Stream a file of known `size` to `dest` with progress, retrying interrupted
/// downloads, rate limits and server errors. `not_found` is the error for HTTP 404.
pub(super) async fn download_file(
    url: &str,
    dest: &Path,
    size: u64,
    not_found: &str,
    say: &(dyn Fn(&str) + Send + Sync),
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let client = http_client()?;
    let mut attempt = 1;
    loop {
        match download_once(&client, url, dest, size, not_found, say).await {
            Ok(()) => return Ok(()),
            Err(DownloadError::Retry(err)) if attempt < DOWNLOAD_ATTEMPTS => {
                say(&format!(
                    "  {err:#}; retrying ({attempt}/{DOWNLOAD_ATTEMPTS})..."
                ));
                tokio::time::sleep(retry_delay(None, None, attempt).unwrap_or_default()).await;
                attempt += 1;
            }
            Err(DownloadError::Retry(err) | DownloadError::Fatal(err)) => {
                let _ = std::fs::remove_file(dest);
                return Err(err);
            }
        }
    }
}

/// A file's size and hex SHA-256, read in chunks.
pub(super) fn file_sha256(path: &Path) -> Result<(u64, String)> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut size = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

/// Check the archive's size and SHA-256 against its manifest.
pub(super) fn verify_archive(archive: &Path, manifest: &BundleManifest) -> Result<()> {
    let (size, digest) = file_sha256(archive)?;
    if size != manifest.size || !digest.eq_ignore_ascii_case(&manifest.sha256) {
        bail!(
            "The Hermes bundle failed verification (expected {} bytes with SHA-256 {}, \
             got {size} bytes with {digest}). Nothing was installed.",
            manifest.size,
            manifest.sha256
        );
    }
    Ok(())
}

/// Upstream's per-install state folder name: the first 16 hex digits of the SHA-256 of
/// the resolved checkout path (`pm/environments.py: install_key`).
pub(super) fn install_key(checkout: &str) -> String {
    hex::encode(Sha256::digest(checkout.as_bytes()))[..16].to_string()
}

/// A path as Python's `Path.resolve()` prints it: canonical, without Windows' `\\?\`.
pub(super) fn resolved_display(path: &Path) -> Result<String> {
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("Failed to resolve {}", path.display()))?;
    let text = canonical.to_string_lossy().into_owned();
    Ok(match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    })
}

/// Old and new spellings of each relocated path. Windows files may hold a path with
/// backslashes, forward slashes, or backslashes escaped for JSON.
fn replacements(
    manifest: &BundleManifest,
    hermes_home: &str,
    home: &str,
    new_key: &str,
    windows: bool,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut push = |old: String, new: String| {
        if windows {
            pairs.push((old.replace('\\', "\\\\"), new.replace('\\', "\\\\")));
            pairs.push((old.replace('\\', "/"), new.replace('\\', "/")));
        }
        pairs.push((old, new));
    };
    let separator = if windows { "\\" } else { "/" };
    push(manifest.build_hermes_home.clone(), hermes_home.to_string());
    // Only the build home's dot-folders (.cache, .local, ...), matching the packager: bare
    // `/Users/runner/...` strings also come from wheels built on other CI runners.
    push(
        format!("{}{separator}.", manifest.build_home),
        format!("{home}{separator}."),
    );
    push(
        format!("installs{separator}{}", manifest.install_key),
        format!("installs{separator}{new_key}"),
    );
    pairs.retain(|(old, new)| !old.is_empty() && old != new);
    // At each position the longest spelling wins, so a home folder never matches inside
    // the Hermes home it contains.
    pairs.sort_by_key(|pair| std::cmp::Reverse(pair.0.len()));
    pairs.dedup();
    pairs
}

/// Replace every occurrence in one left-to-right pass, so replaced text is never
/// rescanned. Windows paths compare without regard to ASCII case.
fn replace_all(text: &str, pairs: &[(String, String)], ignore_case: bool) -> String {
    let haystack = if ignore_case {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    };
    let needles: Vec<String> = pairs
        .iter()
        .map(|(old, _)| {
            if ignore_case {
                old.to_ascii_lowercase()
            } else {
                old.clone()
            }
        })
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    'scan: while index < text.len() {
        for (needle, (_, new)) in needles.iter().zip(pairs) {
            if haystack[index..].starts_with(needle.as_str()) {
                out.push_str(new);
                index += needle.len();
                continue 'scan;
            }
        }
        let ch = text[index..]
            .chars()
            .next()
            .expect("index is a char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// Upstream selects its package-manager runtime by `sha256(pm/pyproject.toml, pm/uv.lock,
/// store Python path)` (`pm/runtime.py: _inputs`), so a moved runtime would be rebuilt from
/// downloads. Re-key it for the moved Python, but only when the same formula reproduces the
/// build's recorded value; otherwise leave it for upstream to rebuild.
fn rekey_pm_runtime(
    stage: &Path,
    manifest: &BundleManifest,
    hermes_home: &str,
    windows: bool,
) -> Result<()> {
    let runtime = stage
        .join("installs")
        .join(&manifest.install_key)
        .join("pm-runtime");
    let selected_path = runtime.join("selected.json");
    let Ok(contents) = std::fs::read_to_string(&selected_path) else {
        return Ok(());
    };
    let mut selected: serde_json::Value = serde_json::from_str(&contents)
        .context("Invalid pm-runtime/selected.json in the bundle")?;
    let pm = stage.join("hermes-agent").join("pm");
    let mut project = Sha256::new();
    for name in ["pyproject.toml", "uv.lock"] {
        project.update(
            std::fs::read(pm.join(name))
                .with_context(|| format!("The bundle is missing pm/{name}"))?,
        );
        project.update(b"\0");
    }
    let (separator, python) = if windows {
        ("\\", manifest.store_python.replace('/', "\\"))
    } else {
        ("/", manifest.store_python.clone())
    };
    let identity = |home: &str| {
        let mut digest = project.clone();
        digest.update(format!("{home}{separator}{python}").as_bytes());
        hex::encode(digest.finalize())
    };
    let old = identity(&manifest.build_hermes_home);
    if selected["inputs"].as_str() != Some(old.as_str()) {
        return Ok(());
    }
    let new = identity(hermes_home);
    selected["inputs"] = serde_json::Value::String(new.clone());
    std::fs::write(
        &selected_path,
        serde_json::to_string_pretty(&selected)? + "\n",
    )
    .context("Failed to re-key the Hermes package-manager runtime")?;
    // The selected generation's own marker records the same identity.
    let Some(generation) = selected["generation"].as_str() else {
        return Ok(());
    };
    let marker = Path::new(generation)
        .components()
        .try_fold(runtime.clone(), |path, component| match component {
            Component::Normal(part) => Some(path.join(part)),
            _ => None,
        })
        .map(|path| path.join("pm-runtime.json"));
    if let Some(marker) = marker {
        if let Ok(text) = std::fs::read_to_string(&marker) {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
                if value["inputs"].as_str() == Some(old.as_str()) {
                    value["inputs"] = serde_json::Value::String(new);
                    std::fs::write(&marker, serde_json::to_string_pretty(&value)? + "\n")
                        .context("Failed to re-key the Hermes package-manager runtime")?;
                }
            }
        }
    }
    Ok(())
}

/// Point the staged software at this computer's Hermes home.
fn relocate(
    stage: &Path,
    manifest: &BundleManifest,
    hermes_home: &str,
    home: &str,
    new_key: &str,
    windows: bool,
) -> Result<()> {
    let pairs = replacements(manifest, hermes_home, home, new_key, windows);
    if !pairs.is_empty() {
        for name in &manifest.relocate {
            let path = stage.join(software_path(name)?);
            let meta = std::fs::symlink_metadata(&path)
                .with_context(|| format!("The bundle is missing {name}, listed for relocation"))?;
            if !meta.is_file() {
                bail!("The bundle lists {name} for relocation, but it is not a regular file");
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {name} for relocation"))?;
            let relocated = replace_all(&text, &pairs, windows);
            if relocated != text {
                std::fs::write(&path, relocated)
                    .with_context(|| format!("Failed to relocate {name}"))?;
            }
        }
    }
    rekey_pm_runtime(stage, manifest, hermes_home, windows)?;
    if manifest.install_key != new_key {
        let installs = stage.join("installs");
        std::fs::rename(installs.join(&manifest.install_key), installs.join(new_key))
            .context("Failed to rename the Hermes install state for this computer")?;
    }
    Ok(())
}

fn extract_into(archive: &Path, stage: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)
        .with_context(|| format!("Failed to open {}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)));
    let mut entries = 0usize;
    let mut total: u64 = 0;
    for entry in tar.entries().context("Failed to read the Hermes bundle")? {
        let mut entry = entry.context("Failed to read the Hermes bundle")?;
        let name = entry
            .path()
            .context("Invalid path in the Hermes bundle")?
            .into_owned();
        let relative = software_path(&name.to_string_lossy())?;
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Directory => {}
            tar::EntryType::Symlink => {
                let target = entry
                    .link_name()?
                    .with_context(|| format!("Bundle link without a target: {}", name.display()))?;
                check_symlink(&relative, &target)?;
            }
            tar::EntryType::Link => {
                let target = entry
                    .link_name()?
                    .with_context(|| format!("Bundle link without a target: {}", name.display()))?;
                software_path(&target.to_string_lossy())?;
            }
            other => bail!(
                "Special file in the Hermes bundle: {} ({other:?})",
                name.display()
            ),
        }
        entries += 1;
        total += entry.size();
        if entries > MAX_ENTRIES || total > MAX_EXTRACTED_BYTES {
            bail!("The Hermes bundle is too large when extracted.");
        }
        if !entry
            .unpack_in(stage)
            .with_context(|| format!("Failed to extract {}", name.display()))?
        {
            bail!(
                "Refusing to extract {} outside the staging folder",
                name.display()
            );
        }
    }
    Ok(())
}

/// Deletes a partly extracted bundle unless it was moved into place.
struct Stage(PathBuf);

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Verify, extract and relocate the bundle into `hermes_home`. The software folders must
/// not exist yet; on any failure nothing is left behind.
pub(super) fn install_bundle(
    archive: &Path,
    manifest: &BundleManifest,
    hermes_home: &Path,
    home: &Path,
    windows: bool,
) -> Result<()> {
    verify_archive(archive, manifest)?;
    for root in ROOTS {
        let path = hermes_home.join(root);
        if path.exists() || path.is_symlink() {
            bail!(
                "Refusing to overwrite existing Hermes software at {}",
                path.display()
            );
        }
    }
    std::fs::create_dir_all(hermes_home)
        .with_context(|| format!("Failed to create {}", hermes_home.display()))?;
    let stage = Stage(hermes_home.join(format!(".clawmacdo-bundle-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&stage.0)
        .with_context(|| format!("Failed to create {}", stage.0.display()))?;
    extract_into(archive, &stage.0)?;
    for root in ROOTS {
        if !stage.0.join(root).is_dir() {
            bail!("The Hermes bundle is missing {root}");
        }
    }
    if !stage
        .0
        .join("hermes-agent/hermes_cli/web_dist/index.html")
        .is_file()
    {
        bail!("The Hermes bundle is missing the compiled dashboard");
    }
    if !stage
        .0
        .join(software_path(&manifest.store_python)?)
        .is_file()
    {
        bail!(
            "The Hermes bundle is missing its Python ({})",
            manifest.store_python
        );
    }

    let resolved_home = resolved_display(hermes_home)?;
    let separator = if windows { "\\" } else { "/" };
    let new_key = install_key(&format!("{resolved_home}{separator}hermes-agent"));
    relocate(
        &stage.0,
        manifest,
        &resolved_home,
        &resolved_display(home)?,
        &new_key,
        windows,
    )?;

    let mut moved: Vec<&str> = Vec::new();
    for root in ROOTS {
        if let Err(err) = std::fs::rename(stage.0.join(root), hermes_home.join(root)) {
            for done in moved.iter().rev() {
                let _ = std::fs::rename(hermes_home.join(done), stage.0.join(done));
            }
            return Err(err)
                .with_context(|| format!("Failed to move {root} into {}", hermes_home.display()));
        }
        moved.push(root);
    }
    Ok(())
}

/// Move existing Hermes software folders into `backup` before installing the bundle.
/// Data (config, `.env`, sessions, memories, skills) stays in place.
pub(super) fn move_existing_software(hermes_home: &Path, backup: &Path) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    for root in ROOTS {
        let path = hermes_home.join(root);
        if !(path.exists() || path.is_symlink()) {
            continue;
        }
        std::fs::create_dir_all(backup)
            .with_context(|| format!("Failed to create {}", backup.display()))?;
        if let Err(err) = std::fs::rename(&path, backup.join(root)) {
            restore_existing_software(hermes_home, backup, &moved);
            return Err(err).with_context(|| {
                format!(
                    "Failed to move the existing {} aside to {}. Stop any running Hermes \
                     processes and try again.",
                    path.display(),
                    backup.display()
                )
            });
        }
        moved.push(root.to_string());
    }
    Ok(moved)
}

/// Undo [`move_existing_software`] after a failed install.
pub(super) fn restore_existing_software(hermes_home: &Path, backup: &Path, moved: &[String]) {
    for root in moved.iter().rev() {
        let current = hermes_home.join(root);
        if current.exists() || current.is_symlink() {
            let _ = std::fs::remove_dir_all(&current);
        }
        let _ = std::fs::rename(backup.join(root), &current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> BundleManifest {
        BundleManifest {
            schema: 1,
            release: "hermes-bundle-v0.22.5-2".into(),
            platform: "macos-arm64".into(),
            filename: "hermes-macos-arm64.tar.gz".into(),
            sha256: "a".repeat(64),
            size: 1024,
            hermes_commit: "bf0203a794a07aac3ff764972ab9087baa4e4f4e".into(),
            hermes_version: "0.22.5".into(),
            build_home: "/Users/runner".into(),
            build_hermes_home: "/Users/runner/.hermes".into(),
            install_key: "0123456789abcdef".into(),
            store_python: "tools/python-3.14.9/bin/python3".into(),
            relocate: vec!["installs/0123456789abcdef/facts.json".into()],
        }
    }

    fn expected() -> ExpectedBundle<'static> {
        ExpectedBundle {
            release: "hermes-bundle-v0.22.5-2",
            platform: "macos-arm64",
            hermes_commit: "bf0203a794a07aac3ff764972ab9087baa4e4f4e",
            hermes_version: "0.22.5",
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("clawmacdo-bundle-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn install_key_matches_upstream_for_a_real_macos_install() {
        assert_eq!(
            install_key("/Users/kennethphang/.hermes/hermes-agent"),
            "3daa46fffbd74d1d"
        );
    }

    #[test]
    fn manifest_must_match_the_pinned_release_platform_and_hermes() {
        validate_manifest(&manifest(), &expected()).unwrap();
        let cases: Vec<Box<dyn Fn(&mut BundleManifest)>> = vec![
            Box::new(|m| m.release = "hermes-bundle-v0.22.5-1".into()),
            Box::new(|m| m.platform = "windows-x64".into()),
            Box::new(|m| m.hermes_commit = "0".repeat(40)),
            Box::new(|m| m.sha256 = "not-a-checksum".into()),
            Box::new(|m| m.size = 5 * 1024 * 1024 * 1024),
            Box::new(|m| m.size = 0),
            Box::new(|m| m.install_key = "xyz".into()),
            Box::new(|m| m.store_python = "../../bin/python3".into()),
            Box::new(|m| m.store_python = "hermes-agent/python3".into()),
            Box::new(|m| m.relocate = vec!["/etc/passwd".into()]),
            Box::new(|m| m.relocate = vec![".ssh/config".into()]),
            Box::new(|m| {
                m.relocate = vec![
                    "tools/python-3.14.9/lib/python3.14/_sysconfig_vars__darwin_darwin.json".into(),
                ]
            }),
        ];
        for change in cases {
            let mut bad = manifest();
            change(&mut bad);
            assert!(validate_manifest(&bad, &expected()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn software_paths_and_links_stay_inside_the_bundle_folders() {
        assert!(software_path("hermes-agent/run_agent.py").is_ok());
        assert!(software_path("tools").is_ok());
        for bad in [
            "/tools/x",
            "../tools",
            "tools/../../x",
            "skills/x",
            ".env",
            "",
        ] {
            assert!(software_path(bad).is_err(), "{bad}");
        }
        let entry = Path::new("hermes-agent/node_modules/.bin/vite");
        assert!(check_symlink(entry, Path::new("../vite/bin/vite.js")).is_ok());
        assert!(check_symlink(entry, Path::new("../../../tools/node/bin/node")).is_ok());
        assert!(check_symlink(entry, Path::new("../../../../.ssh/id_rsa")).is_err());
        assert!(check_symlink(entry, Path::new("/usr/bin/env")).is_err());
    }

    #[test]
    fn downloads_retry_rate_limits_server_errors_and_network_failures_only() {
        use reqwest::StatusCode;
        use std::time::Duration;
        let wait = |status, retry_after, attempt| retry_delay(status, retry_after, attempt);
        assert_eq!(
            wait(Some(StatusCode::TOO_MANY_REQUESTS), None, 1),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            wait(Some(StatusCode::TOO_MANY_REQUESTS), Some("12"), 1),
            Some(Duration::from_secs(12))
        );
        assert_eq!(
            wait(Some(StatusCode::TOO_MANY_REQUESTS), Some("3600"), 1),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            wait(Some(StatusCode::BAD_GATEWAY), None, 2),
            Some(Duration::from_secs(20))
        );
        assert_eq!(wait(None, None, 3), Some(Duration::from_secs(45)));
        assert_eq!(wait(None, None, DOWNLOAD_ATTEMPTS), None);
        for status in [
            StatusCode::NOT_FOUND,
            StatusCode::FORBIDDEN,
            StatusCode::UNAUTHORIZED,
        ] {
            assert_eq!(wait(Some(status), None, 1), None, "{status}");
        }
    }

    #[test]
    fn missing_bundle_message_offers_from_source() {
        let message = unavailable_message("https://example.test/b.tar.gz.json", "macos-arm64");
        assert!(message.contains("--from-source"));
        assert!(message.contains("Nothing was changed"));
    }

    #[test]
    fn replacement_prefers_the_longest_path_and_never_rescans_output() {
        let pairs = replacements(
            &manifest(),
            "/Users/runner2/.hermes",
            "/Users/runner2",
            "fedcba9876543210",
            false,
        );
        let text = "home = /Users/runner/.hermes/tools/python\n\
                    env = /Users/runner/.hermes/installs/0123456789abcdef/venv\n\
                    cache = /Users/runner/.cache/uv\n\
                    wheel = /Users/runner/work/pillow/src\n";
        assert_eq!(
            replace_all(text, &pairs, false),
            "home = /Users/runner2/.hermes/tools/python\n\
             env = /Users/runner2/.hermes/installs/fedcba9876543210/venv\n\
             cache = /Users/runner2/.cache/uv\n\
             wheel = /Users/runner/work/pillow/src\n"
        );
        // Installing into the build's own home changes nothing.
        assert!(replacements(
            &manifest(),
            "/Users/runner/.hermes",
            "/Users/runner",
            "0123456789abcdef",
            false
        )
        .is_empty());
    }

    #[test]
    fn windows_replacement_covers_json_escaped_slashed_and_differently_cased_paths() {
        let mut m = manifest();
        m.build_home = r"C:\Users\runneradmin".into();
        m.build_hermes_home = r"C:\Users\runneradmin\AppData\Local\hermes".into();
        let pairs = replacements(
            &m,
            r"D:\relocated\AppData\Local\hermes",
            r"C:\Users\alice",
            "fedcba9876543210",
            true,
        );
        let text = concat!(
            r#"{"env": "C:\\Users\\runneradmin\\AppData\\Local\\hermes\\installs\\0123456789abcdef\\venv"}"#,
            "\n",
            r"home = c:\users\runneradmin\appdata\local\hermes\tools\python",
            "\n",
            "root = C:/Users/runneradmin/AppData/Local/hermes/hermes-agent\n",
            r"profile = C:\Users\runneradmin\.cache",
        );
        assert_eq!(
            replace_all(text, &pairs, true),
            concat!(
                r#"{"env": "D:\\relocated\\AppData\\Local\\hermes\\installs\\fedcba9876543210\\venv"}"#,
                "\n",
                r"home = D:\relocated\AppData\Local\hermes\tools\python",
                "\n",
                "root = D:/relocated/AppData/Local/hermes/hermes-agent\n",
                r"profile = C:\Users\alice\.cache",
            )
        );
    }

    fn write_bundle(dir: &Path, entries: &[(&str, &[u8])]) -> (PathBuf, BundleManifest) {
        let archive = dir.join("hermes-macos-arm64.tar.gz");
        let file = std::fs::File::create(&archive).unwrap();
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::fast(),
        ));
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        let bytes = std::fs::read(&archive).unwrap();
        let mut m = manifest();
        m.size = bytes.len() as u64;
        m.sha256 = hex::encode(Sha256::digest(&bytes));
        (archive, m)
    }

    #[cfg(unix)]
    #[test]
    fn bundle_installs_relocates_and_rekeys_the_install_state() {
        let dir = TempDir::new();
        let (archive, mut m) = write_bundle(
            &dir.0,
            &[
                ("hermes-agent/hermes_cli/web_dist/index.html", b"<html>"),
                ("tools/python-3.14.9/bin/python3", b"#!python"),
                (
                    "installs/0123456789abcdef/facts.json",
                    b"{\"venv\": \"/Users/runner/.hermes/installs/0123456789abcdef/venv\"}",
                ),
            ],
        );
        let home = dir.0.join("home");
        let hermes_home = home.join(".hermes");
        std::fs::create_dir_all(&hermes_home).unwrap();
        install_bundle(&archive, &m, &hermes_home, &home, false).unwrap();

        let resolved = resolved_display(&hermes_home).unwrap();
        let key = install_key(&format!("{resolved}/hermes-agent"));
        let facts =
            std::fs::read_to_string(hermes_home.join("installs").join(&key).join("facts.json"))
                .unwrap();
        assert_eq!(
            facts,
            format!("{{\"venv\": \"{resolved}/installs/{key}/venv\"}}")
        );
        assert!(!hermes_home.join("installs/0123456789abcdef").exists());
        assert!(hermes_home
            .join("tools/python-3.14.9/bin/python3")
            .is_file());
        assert_eq!(
            std::fs::read_dir(&hermes_home).unwrap().count(),
            3,
            "the staging folder is removed"
        );

        // Existing software is never overwritten.
        let err = install_bundle(&archive, &m, &hermes_home, &home, false).unwrap_err();
        assert!(err.to_string().contains("Refusing to overwrite"), "{err:#}");

        // A tampered archive is rejected before anything is extracted.
        let fresh = dir.0.join("fresh");
        m.sha256 = "0".repeat(64);
        let err = install_bundle(&archive, &m, &fresh, &home, false).unwrap_err();
        assert!(err.to_string().contains("failed verification"), "{err:#}");
        assert!(!fresh.join("tools").exists());
    }

    #[test]
    fn incomplete_bundles_leave_nothing_behind() {
        let dir = TempDir::new();
        let (archive, m) = write_bundle(&dir.0, &[("tools/python-3.14.9/bin/python3", b"py")]);
        let hermes_home = dir.0.join("hermes");
        let err = install_bundle(&archive, &m, &hermes_home, &dir.0, false).unwrap_err();
        assert!(err.to_string().contains("missing hermes-agent"), "{err:#}");
        assert_eq!(std::fs::read_dir(&hermes_home).unwrap().count(), 0);

        let (archive, m) = write_bundle(&dir.0, &[(".env", b"OPENROUTER_API_KEY=x")]);
        let err = install_bundle(&archive, &m, &hermes_home, &dir.0, false).unwrap_err();
        assert!(
            err.to_string().contains("outside the Hermes software"),
            "{err:#}"
        );
        assert_eq!(std::fs::read_dir(&hermes_home).unwrap().count(), 0);
    }

    /// `pm/runtime.py: _inputs`, written out independently of the code under test.
    fn runtime_identity(pyproject: &str, lock: &str, python: &str) -> String {
        hex::encode(Sha256::digest(
            format!("{pyproject}\0{lock}\0{python}").as_bytes(),
        ))
    }

    #[test]
    fn package_manager_runtime_is_rekeyed_only_when_the_formula_matches() {
        let dir = TempDir::new();
        let stage = &dir.0;
        let pm = stage.join("hermes-agent/pm");
        std::fs::create_dir_all(&pm).unwrap();
        std::fs::write(pm.join("pyproject.toml"), "[project]\n").unwrap();
        std::fs::write(pm.join("uv.lock"), "version = 1\n").unwrap();
        let runtime = stage.join("installs/0123456789abcdef/pm-runtime");
        let generation = runtime.join("generations/abc");
        std::fs::create_dir_all(&generation).unwrap();
        let m = manifest();
        let old = runtime_identity(
            "[project]\n",
            "version = 1\n",
            "/Users/runner/.hermes/tools/python-3.14.9/bin/python3",
        );
        let selected = serde_json::json!({"generation": "generations/abc", "inputs": old});
        std::fs::write(runtime.join("selected.json"), selected.to_string()).unwrap();
        std::fs::write(
            generation.join("pm-runtime.json"),
            serde_json::json!({ "inputs": old }).to_string(),
        )
        .unwrap();

        rekey_pm_runtime(stage, &m, "/Users/alice/.hermes", false).unwrap();
        let new = runtime_identity(
            "[project]\n",
            "version = 1\n",
            "/Users/alice/.hermes/tools/python-3.14.9/bin/python3",
        );
        let read = |path: PathBuf| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        assert_eq!(read(runtime.join("selected.json"))["inputs"], new);
        assert_eq!(
            read(runtime.join("selected.json"))["generation"],
            "generations/abc"
        );
        assert_eq!(read(generation.join("pm-runtime.json"))["inputs"], new);

        // A record this formula does not reproduce is left for upstream to rebuild.
        let foreign =
            serde_json::json!({"generation": "generations/abc", "inputs": "f".repeat(64)});
        std::fs::write(runtime.join("selected.json"), foreign.to_string()).unwrap();
        rekey_pm_runtime(stage, &m, "/Users/bob/.hermes", false).unwrap();
        assert_eq!(
            read(runtime.join("selected.json"))["inputs"],
            "f".repeat(64)
        );
    }

    #[test]
    fn existing_software_moves_aside_and_can_be_restored() {
        let dir = TempDir::new();
        let hermes_home = dir.0.join("hermes");
        std::fs::create_dir_all(hermes_home.join("hermes-agent")).unwrap();
        std::fs::create_dir_all(hermes_home.join("tools")).unwrap();
        std::fs::write(hermes_home.join("config.yaml"), "model: {}\n").unwrap();
        let backup = dir.0.join("backup/software");

        let moved = move_existing_software(&hermes_home, &backup).unwrap();
        assert_eq!(moved, ["hermes-agent", "tools"]);
        assert!(!hermes_home.join("hermes-agent").exists());
        assert!(hermes_home.join("config.yaml").is_file(), "data stays put");
        assert!(backup.join("tools").is_dir());

        std::fs::create_dir_all(hermes_home.join("tools/partial")).unwrap();
        restore_existing_software(&hermes_home, &backup, &moved);
        assert!(hermes_home.join("hermes-agent").is_dir());
        assert!(!hermes_home.join("tools/partial").exists());
    }
}
