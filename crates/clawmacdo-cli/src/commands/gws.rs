//! Google Workspace (`gws`) credential management on a deployed OpenClaw instance.
//!
//! `gws auth login` is an interactive, browser-based OAuth flow that spins up a
//! local loopback callback server on a random port — it has no headless / paste-
//! the-code mode (see googleworkspace/cli#210). A deployed instance is headless,
//! and all of our remote execution is one-shot, non-interactive SSH, so we cannot
//! drive that flow from here.
//!
//! Instead we use the credential-injection model: an external system (e.g.
//! 2ndbrain.ceo) performs the Google OAuth in a real browser, and the resulting
//! credentials JSON is pushed into `~/.config/gws/` on the instance so the
//! agent's `gws` CLI is authenticated. Logout runs `gws auth logout` (revoke +
//! clear) with a local-file-removal fallback.

use anyhow::{bail, Context, Result};
use base64::Engine;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use std::path::PathBuf;

/// Look up a deploy record by hostname, IP, or deploy ID.
/// Returns (ip, ssh_key_path, provider).
fn find_deploy_record(query: &str) -> Result<(String, PathBuf, Option<String>)> {
    let deploys_dir = config::deploys_dir()?;
    if !deploys_dir.exists() {
        bail!("No deploy records found. Deploy an instance first.");
    }

    for entry in std::fs::read_dir(&deploys_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)?;
        let record: config::DeployRecord = match serde_json::from_str(&contents) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if record.id == query || record.hostname == query || record.ip_address == query {
            let provider = record.provider.map(|p| p.to_string());
            return Ok((
                record.ip_address,
                PathBuf::from(record.ssh_key_path),
                provider,
            ));
        }
    }

    bail!("No deploy record found for '{query}'. Use a deploy ID, hostname, or IP address.");
}

fn ssh_user_for_provider(provider: &Option<String>) -> &'static str {
    match provider.as_deref() {
        Some("lightsail") => "ubuntu",
        _ => "root",
    }
}

/// Validate that `name` is a bare file name suitable for writing under
/// `~/.config/gws/` — no path separators or traversal that could escape the dir.
fn validate_dest_filename(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        bail!("Invalid --filename '{name}': must be a bare file name (e.g. credentials.json)");
    }
    Ok(())
}

/// Where the credentials to install come from.
pub enum LoginSource {
    /// A local JSON file (e.g. from `gws auth export --unmasked`).
    File(PathBuf),
    /// An OAuth authorization code to exchange with Google for credentials.
    Code {
        code: String,
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        /// PKCE verifier, required only if the auth request used PKCE.
        code_verifier: Option<String>,
    },
}

impl LoginSource {
    /// Build a [`LoginSource`] from the raw `gws-login` flags, enforcing that
    /// exactly one of `--credentials` / `--code` is supplied and that `--code`
    /// carries its required companions.
    pub fn from_args(
        credentials: Option<PathBuf>,
        code: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        redirect_uri: Option<String>,
        code_verifier: Option<String>,
    ) -> Result<Self> {
        match (credentials, code) {
            (Some(_), Some(_)) => {
                bail!("Pass either --credentials <file> or --code <oauth-code>, not both.")
            }
            (None, None) => bail!(
                "Provide credentials with --credentials <file>, or --code <oauth-code> together \
                 with --client-id, --client-secret and --redirect-uri."
            ),
            (Some(path), None) => Ok(LoginSource::File(path)),
            (None, Some(code)) => Ok(LoginSource::Code {
                code,
                client_id: client_id.context("--code requires --client-id")?,
                client_secret: client_secret.context("--code requires --client-secret")?,
                redirect_uri: redirect_uri.context("--code requires --redirect-uri")?,
                code_verifier,
            }),
        }
    }
}

/// Exchange an OAuth authorization code for a gws `authorized_user` credentials
/// JSON by calling Google's token endpoint.
///
/// gws reads `~/.config/gws/credentials.json` as a fallback source; the file it
/// expects is the standard Google `authorized_user` shape
/// (`type`/`client_id`/`client_secret`/`refresh_token`). We build exactly that
/// from the token response so gws can load it with no re-export step.
async fn exchange_code(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code_verifier: Option<&str>,
) -> Result<Vec<u8>> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect_uri),
    ];
    if let Some(verifier) = code_verifier {
        form.push(("code_verifier", verifier));
    }

    let resp = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&form)
        .send()
        .await
        .context("requesting token from https://oauth2.googleapis.com/token")?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .context("reading Google token response body")?;

    if !status.is_success() {
        bail!(
            "Google token endpoint returned {status}: {}\n  \
             Common causes: the code is expired or already used (codes are single-use and live \
             ~10 minutes), --redirect-uri does not exactly match the one used to obtain the \
             code, or the flow used PKCE but --code-verifier was not supplied.",
            body.trim()
        );
    }

    let token: serde_json::Value =
        serde_json::from_str(&body).context("parsing Google token response as JSON")?;

    let refresh_token = token
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .context(
            "Google returned an access token but no refresh_token, so the agent would lose \
             access within ~1 hour.\n  The authorization request must use access_type=offline \
             (and typically prompt=consent). Re-run the OAuth flow with offline access enabled, \
             then retry gws-login with a fresh code.",
        )?;

    let creds = serde_json::json!({
        "type": "authorized_user",
        "client_id": client_id,
        "client_secret": client_secret,
        "refresh_token": refresh_token,
    });

    serde_json::to_vec_pretty(&creds).context("serializing authorized_user credentials")
}

/// Install Google Workspace credentials on a deployed instance.
///
/// Obtains the credentials JSON from `source` — either a local file (validated as
/// JSON) or by exchanging an OAuth code with Google — then writes it to
/// `~/.config/gws/<filename>` on the instance as the `openclaw` user with `0600`
/// permissions. The payload is base64-encoded so JSON quoting can't break the
/// remote shell.
pub async fn login(query: &str, source: LoginSource, filename: &str) -> Result<()> {
    validate_dest_filename(filename)?;

    let creds: Vec<u8> = match source {
        LoginSource::File(path) => {
            if !path.exists() {
                bail!("Credentials file not found: {}", path.display());
            }
            let creds =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            if creds.is_empty() {
                bail!("Credentials file is empty: {}", path.display());
            }
            // Validate it parses as JSON so we don't push garbage to the instance.
            serde_json::from_slice::<serde_json::Value>(&creds)
                .with_context(|| format!("{} is not valid JSON", path.display()))?;
            creds
        }
        LoginSource::Code {
            code,
            client_id,
            client_secret,
            redirect_uri,
            code_verifier,
        } => {
            println!("Exchanging authorization code for gws credentials with Google ...");
            exchange_code(
                &code,
                &client_id,
                &client_secret,
                &redirect_uri,
                code_verifier.as_deref(),
            )
            .await?
        }
    };

    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;
    let gws_dir = format!("{home}/.config/gws");
    let dest = format!("{gws_dir}/{filename}");

    let b64 = base64::engine::general_purpose::STANDARD.encode(&creds);

    println!(
        "Pushing gws credentials ({} bytes) to {ip}:{dest} ...",
        creds.len()
    );

    // One SSH session as the openclaw user:
    //  1. write the credentials into ~/.config/gws with 0600 perms (umask 077),
    //  2. confirm it landed,
    //  3. best-effort `gws auth status` so the caller sees gws accepted it.
    let script = format!(
        "set -e\n\
         umask 077\n\
         mkdir -p '{gws_dir}'\n\
         printf '%s' '{b64}' | base64 -d > '{dest}.tmp'\n\
         chmod 600 '{dest}.tmp'\n\
         mv -f '{dest}.tmp' '{dest}'\n\
         echo __GWS_CREDS_INSTALLED__\n\
         ls -l '{dest}'\n\
         export PATH=\"{home}/.local/bin:/usr/local/bin:/usr/bin:/bin\" HOME='{home}' GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND=file\n\
         if [ -x '{home}/.local/bin/gws' ]; then\n\
           '{home}/.local/bin/gws' auth status 2>&1 || echo '(gws auth status unavailable — file placed; gws will read it at runtime)'\n\
         else\n\
           echo '(gws binary not found at {home}/.local/bin/gws — file placed for when gws is installed)'\n\
         fi\n"
    );

    let out = ssh_as_openclaw_with_user_async(&ip, &key, &script, ssh_user).await?;

    if !out.contains("__GWS_CREDS_INSTALLED__") {
        bail!("Failed to install gws credentials on {ip}:\n{}", out.trim());
    }
    for line in out.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() || l.contains("__GWS_CREDS_INSTALLED__") {
            continue;
        }
        println!("  {l}");
    }

    println!("\ngws credentials installed at {dest} on {ip}.");
    println!("The OpenClaw agent will use them next time it runs `gws`.");
    Ok(())
}

/// Log out Google Workspace on a deployed instance.
///
/// Runs `gws auth logout` (revokes the token with Google and clears it), then
/// removes any local `credentials.json` / `token.json` as a fallback. Keeps
/// `client_secret.json` so a future login needs no `gws auth setup` re-run.
pub async fn logout(query: &str) -> Result<()> {
    let (ip, key, provider) = find_deploy_record(query)?;
    let ssh_user = ssh_user_for_provider(&provider);
    let home = config::OPENCLAW_HOME;
    let gws_dir = format!("{home}/.config/gws");

    println!("Logging out gws on {ip} ...");

    let script = format!(
        "export PATH=\"{home}/.local/bin:/usr/local/bin:/usr/bin:/bin\" HOME='{home}' GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND=file\n\
         if [ -x '{home}/.local/bin/gws' ]; then\n\
           if '{home}/.local/bin/gws' auth logout 2>&1; then\n\
             echo __GWS_LOGOUT_OK__\n\
           else\n\
             echo __GWS_LOGOUT_FALLBACK__\n\
           fi\n\
         else\n\
           echo __GWS_NO_BINARY__\n\
         fi\n\
         rm -f '{gws_dir}/credentials.json' '{gws_dir}/token.json' 2>/dev/null || true\n\
         echo __GWS_LOCAL_CLEARED__\n"
    );

    let out = ssh_as_openclaw_with_user_async(&ip, &key, &script, ssh_user).await?;

    let revoked = out.contains("__GWS_LOGOUT_OK__");
    let no_binary = out.contains("__GWS_NO_BINARY__");

    // Echo gws's own output, hiding our internal markers.
    for line in out.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("__GWS_") {
            continue;
        }
        println!("  {l}");
    }

    if revoked {
        println!("\ngws session revoked with Google and local credentials cleared on {ip}.");
    } else if no_binary {
        println!("\ngws binary not found; removed any local gws credentials on {ip}.");
    } else {
        println!(
            "\n`gws auth logout` was unavailable or failed; removed local gws credentials \
             (credentials.json/token.json) on {ip}.\n  Note: the token may still be valid with \
             Google until it expires — revoke it at https://myaccount.google.com/permissions if needed."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bare_filenames() {
        assert!(validate_dest_filename("credentials.json").is_ok());
        assert!(validate_dest_filename("token.json").is_ok());
        assert!(validate_dest_filename("client_secret.json").is_ok());
    }

    #[test]
    fn rejects_path_separators_and_traversal() {
        assert!(validate_dest_filename("").is_err());
        assert!(validate_dest_filename("../credentials.json").is_err());
        assert!(validate_dest_filename("sub/dir.json").is_err());
        assert!(validate_dest_filename("a\\b.json").is_err());
        assert!(validate_dest_filename("..").is_err());
    }

    #[test]
    fn from_args_accepts_credentials_file() {
        let src = LoginSource::from_args(
            Some(PathBuf::from("creds.json")),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("file source");
        assert!(matches!(src, LoginSource::File(_)));
    }

    #[test]
    fn from_args_accepts_code_with_required_peers() {
        let src = LoginSource::from_args(
            None,
            Some("auth-code".into()),
            Some("cid".into()),
            Some("secret".into()),
            Some("https://example.com/cb".into()),
            Some("verifier".into()),
        )
        .expect("code source");
        match src {
            LoginSource::Code {
                code,
                client_id,
                code_verifier,
                ..
            } => {
                assert_eq!(code, "auth-code");
                assert_eq!(client_id, "cid");
                assert_eq!(code_verifier.as_deref(), Some("verifier"));
            }
            _ => panic!("expected Code source"),
        }
    }

    #[test]
    fn from_args_rejects_both_sources() {
        assert!(LoginSource::from_args(
            Some(PathBuf::from("creds.json")),
            Some("code".into()),
            None,
            None,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn from_args_rejects_neither_source() {
        assert!(LoginSource::from_args(None, None, None, None, None, None).is_err());
    }

    #[test]
    fn from_args_rejects_code_missing_peers() {
        // --code without --client-id/--client-secret/--redirect-uri must fail.
        assert!(LoginSource::from_args(
            None,
            Some("code".into()),
            None,
            Some("secret".into()),
            Some("https://example.com/cb".into()),
            None,
        )
        .is_err());
    }
}
