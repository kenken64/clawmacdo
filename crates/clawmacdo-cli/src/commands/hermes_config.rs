//! Hermes Agent configuration shared by the Lightsail and local installers.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

pub const DEFAULT_BEDROCK_REGION: &str = "ap-southeast-1";
pub const DEFAULT_BEDROCK_MODEL: &str = "amazon.nova-pro-v1:0";
pub const DEFAULT_OPENROUTER_MODEL: &str = "openai/gpt-6-sol";
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    OpenRouter,
    Bedrock,
}

/// Where Hermes runs the shell commands the agent issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalBackend {
    /// Directly on the host, with the installing user's permissions.
    Local,
    /// Inside upstream's default Docker/Podman sandbox image.
    Docker,
}

impl TerminalBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            TerminalBackend::Local => "local",
            TerminalBackend::Docker => "docker",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "local" => Ok(TerminalBackend::Local),
            "docker" => Ok(TerminalBackend::Docker),
            other => bail!("Unsupported --terminal-backend '{other}'. Use local or docker."),
        }
    }
}

/// The AI, Telegram, and environment settings every Hermes install writes.
#[derive(Debug, Clone)]
pub struct HermesAgentSettings {
    pub env_file: Option<PathBuf>,
    pub env_content: Option<String>,
    pub env_vars: Vec<String>,
    pub ai_provider: String,
    pub openrouter_api_key: Option<String>,
    pub openrouter_model: String,
    pub bedrock_api_key: Option<String>,
    pub bedrock_region: String,
    pub bedrock_model: String,
    pub telegram_bot_token: Option<String>,
    pub telegram_allowed_users: Option<String>,
    pub telegram_home_channel: Option<String>,
    pub terminal_backend: TerminalBackend,
}

pub(super) fn validate_env_assignment(value: &str) -> Result<()> {
    let Some((key, _)) = value.split_once('=') else {
        bail!("--env values must use KEY=VALUE syntax");
    };
    if key.is_empty() {
        bail!("--env key cannot be empty");
    }
    let mut chars = key.chars();
    let first = chars.next().unwrap_or_default();
    if !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        bail!("Invalid env key '{key}'. Use shell-style names like OPENAI_API_KEY.");
    }
    Ok(())
}

fn append_env_assignment(out: &mut String, key: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(key);
    out.push('=');
    out.push_str(trimmed);
    out.push('\n');
}

fn normalize_csv(value: &str) -> Option<String> {
    let normalized = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn first_csv_value(value: &str) -> Option<String> {
    value
        .split(',')
        .map(str::trim)
        .find(|part| !part.is_empty())
        .map(str::to_string)
}

pub(super) fn normalized_bedrock_region(settings: &HermesAgentSettings) -> String {
    let region = settings.bedrock_region.trim();
    if region.is_empty() {
        DEFAULT_BEDROCK_REGION.to_string()
    } else {
        region.to_string()
    }
}

pub(super) fn normalized_bedrock_model(settings: &HermesAgentSettings) -> String {
    let model = settings.bedrock_model.trim();
    if model.is_empty() {
        DEFAULT_BEDROCK_MODEL.to_string()
    } else {
        model.to_string()
    }
}

pub(super) fn normalized_openrouter_model(settings: &HermesAgentSettings) -> String {
    let model = settings.openrouter_model.trim();
    if model.is_empty() {
        DEFAULT_OPENROUTER_MODEL.to_string()
    } else {
        model.to_string()
    }
}

pub(super) fn has_value(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
}

/// Validate a Telegram bot token of the form `<bot_id>:<secret>`.
pub(super) fn validate_telegram_bot_token(token: &str) -> Result<()> {
    let token = token.trim();
    let Some((bot_id, secret)) = token.split_once(':') else {
        bail!("Invalid Telegram bot token: expected <bot_id>:<secret> from @BotFather.");
    };
    if bot_id.is_empty() || !bot_id.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("Invalid Telegram bot token: the bot ID before ':' must be numeric.");
    }
    if secret.is_empty()
        || !secret
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("Invalid Telegram bot token: the secret after ':' contains unexpected characters.");
    }
    Ok(())
}

pub(super) enum TelegramVerification {
    Verified(String),
    Unverified(String),
}

/// Call Telegram's `getMe` endpoint to confirm the bot token is live.
///
/// An invalid token is a hard error. Network failures return `Unverified` so
/// provisioning can still proceed and be verified later.
pub(super) async fn verify_telegram_bot_token(token: &str) -> Result<TelegramVerification> {
    validate_telegram_bot_token(token)?;
    let url = format!("https://api.telegram.org/bot{}/getMe", token.trim());
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(err) => return Ok(TelegramVerification::Unverified(err.to_string())),
    };
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(err) => return Ok(TelegramVerification::Unverified(err.to_string())),
    };
    let payload: serde_json::Value = match response.json().await {
        Ok(payload) => payload,
        Err(err) => return Ok(TelegramVerification::Unverified(err.to_string())),
    };
    if payload.get("ok").and_then(|value| value.as_bool()) == Some(true) {
        let username = payload
            .pointer("/result/username")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        Ok(TelegramVerification::Verified(username))
    } else {
        let description = payload
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("Telegram rejected the bot token");
        bail!("Telegram bot token verification failed: {description}");
    }
}

/// Resolve which AI provider to configure.
///
/// An explicit `--ai-provider` wins. Otherwise OpenRouter is preferred when
/// an OpenRouter key is supplied, falling back to Bedrock for backwards
/// compatibility with existing `--bedrock-api-key` deployments.
pub(super) fn resolve_ai_provider(settings: &HermesAgentSettings) -> Result<AiProvider> {
    let requested = settings.ai_provider.trim().to_ascii_lowercase();
    let has_openrouter = has_value(settings.openrouter_api_key.as_deref());
    let has_bedrock = has_value(settings.bedrock_api_key.as_deref());

    match requested.as_str() {
        "openrouter" => {
            if !has_openrouter {
                bail!(
                    "OpenRouter provider selected but no API key was supplied. \
                     Pass --openrouter-api-key or set OPENROUTER_API_KEY."
                );
            }
            Ok(AiProvider::OpenRouter)
        }
        "bedrock" | "aws-bedrock" => {
            if !has_bedrock {
                bail!(
                    "Bedrock provider selected but no API key was supplied. \
                     Pass --bedrock-api-key or set AWS_BEARER_TOKEN_BEDROCK."
                );
            }
            Ok(AiProvider::Bedrock)
        }
        "" | "auto" => {
            if has_openrouter {
                Ok(AiProvider::OpenRouter)
            } else if has_bedrock {
                Ok(AiProvider::Bedrock)
            } else {
                bail!(
                    "AI provider credentials required. Set OPENROUTER_API_KEY \
                     (recommended) or AWS_BEARER_TOKEN_BEDROCK."
                );
            }
        }
        other => bail!("Unsupported --ai-provider '{other}'. Use openrouter or bedrock."),
    }
}

pub(super) fn bedrock_mantle_base_url(region: &str) -> String {
    format!("https://bedrock-mantle.{region}.api.aws/v1")
}

fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(super) fn build_bedrock_config_content(
    region: &str,
    model: &str,
    backend: TerminalBackend,
) -> String {
    let base_url = bedrock_mantle_base_url(region);
    format!(
        "model:\n  default: {}\n  provider: custom\n  base_url: {}\nbedrock:\n  region: {}\nterminal:\n  backend: {}\n",
        yaml_single_quote(model),
        yaml_single_quote(&base_url),
        yaml_single_quote(region),
        backend.as_str(),
    )
}

pub(super) fn build_openrouter_config_content(model: &str, backend: TerminalBackend) -> String {
    format!(
        "model:\n  default: {}\n  provider: openrouter\n  base_url: {}\nterminal:\n  backend: {}\n",
        yaml_single_quote(model),
        yaml_single_quote(OPENROUTER_BASE_URL),
        backend.as_str(),
    )
}

/// The Hermes `config.yaml` for the resolved provider, plus a one-line summary.
pub(super) fn build_config_content(settings: &HermesAgentSettings) -> Result<(String, String)> {
    let backend = settings.terminal_backend;
    Ok(match resolve_ai_provider(settings)? {
        AiProvider::OpenRouter => {
            let model = normalized_openrouter_model(settings);
            (
                build_openrouter_config_content(&model, backend),
                format!("OpenRouter {model}"),
            )
        }
        AiProvider::Bedrock => {
            let region = normalized_bedrock_region(settings);
            let model = normalized_bedrock_model(settings);
            (
                build_bedrock_config_content(&region, &model, backend),
                format!("AWS Bedrock {model} ({region})"),
            )
        }
    })
}

/// OpenCode V2 global config that routes the CLI through OpenRouter using the
/// same model as Hermes.
pub(super) fn build_opencode_config_content(model: &str) -> String {
    let payload = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "model": format!("openrouter/{model}"),
        "providers": {
            "openrouter": {
                "env": ["OPENROUTER_API_KEY"],
                "settings": {
                    "apiKey": "{file:~/.config/opencode/openrouter-api-key}",
                },
            },
        },
    });
    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
}

pub(super) fn build_env_content(settings: &HermesAgentSettings) -> Result<String> {
    let mut chunks = Vec::new();

    if let Some(path) = settings.env_file.as_deref() {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read env file {}", path.display()))?;
        chunks.push(contents.trim_end().to_string());
    }

    if let Some(contents) = settings.env_content.as_deref() {
        let trimmed = contents.trim_end();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
    }

    for item in &settings.env_vars {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        validate_env_assignment(trimmed)?;
        chunks.push(trimmed.to_string());
    }

    let mut env = chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !env.is_empty() && !env.ends_with('\n') {
        env.push('\n');
    }
    match resolve_ai_provider(settings)? {
        AiProvider::OpenRouter => {
            if let Some(api_key) = settings.openrouter_api_key.as_deref() {
                append_env_assignment(&mut env, "OPENROUTER_API_KEY", api_key);
            }
        }
        AiProvider::Bedrock => {
            if let Some(api_key) = settings.bedrock_api_key.as_deref() {
                let region = normalized_bedrock_region(settings);
                let base_url = bedrock_mantle_base_url(&region);
                append_env_assignment(&mut env, "AWS_BEARER_TOKEN_BEDROCK", api_key);
                append_env_assignment(&mut env, "OPENAI_API_KEY", api_key);
                append_env_assignment(&mut env, "OPENAI_BASE_URL", &base_url);
                append_env_assignment(&mut env, "AWS_REGION", &region);
                append_env_assignment(&mut env, "AWS_DEFAULT_REGION", &region);
            }
        }
    }
    if let Some(token) = settings.telegram_bot_token.as_deref() {
        append_env_assignment(&mut env, "TELEGRAM_BOT_TOKEN", token);
    }
    let allowed_users = settings
        .telegram_allowed_users
        .as_deref()
        .and_then(normalize_csv);
    if let Some(users) = allowed_users.as_deref() {
        append_env_assignment(&mut env, "TELEGRAM_ALLOWED_USERS", users);
    }
    let home_channel = settings
        .telegram_home_channel
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| allowed_users.as_deref().and_then(first_csv_value));
    if let Some(channel) = home_channel.as_deref() {
        append_env_assignment(&mut env, "TELEGRAM_HOME_CHANNEL", channel);
    }
    append_env_assignment(&mut env, "TERMINAL_ENV", settings.terminal_backend.as_str());
    Ok(env)
}

#[cfg(test)]
pub(super) fn test_settings(
    ai_provider: &str,
    openrouter_api_key: Option<&str>,
    bedrock_api_key: Option<&str>,
) -> HermesAgentSettings {
    HermesAgentSettings {
        env_file: None,
        env_content: None,
        env_vars: Vec::new(),
        ai_provider: ai_provider.to_string(),
        openrouter_api_key: openrouter_api_key.map(str::to_string),
        openrouter_model: DEFAULT_OPENROUTER_MODEL.to_string(),
        bedrock_api_key: bedrock_api_key.map(str::to_string),
        bedrock_region: DEFAULT_BEDROCK_REGION.to_string(),
        bedrock_model: DEFAULT_BEDROCK_MODEL.to_string(),
        telegram_bot_token: None,
        telegram_allowed_users: None,
        telegram_home_channel: None,
        terminal_backend: TerminalBackend::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_repeated_env_assignments() {
        assert!(validate_env_assignment("OPENAI_API_KEY=sk-test").is_ok());
        assert!(validate_env_assignment("_CUSTOM=value").is_ok());
        assert!(validate_env_assignment("1BAD=value").is_err());
        assert!(validate_env_assignment("NO_EQUALS").is_err());
    }

    #[test]
    fn build_env_content_authorizes_telegram_users_directly() {
        let mut settings = test_settings("", None, Some("bedrock-test"));
        settings.env_content = Some("NOUS_API_KEY=nous-test".to_string());
        settings.env_vars = vec!["OPENROUTER_API_KEY=or-test".to_string()];
        settings.telegram_bot_token = Some("123456:abc".to_string());
        settings.telegram_allowed_users = Some("111, 222".to_string());

        let env = build_env_content(&settings).unwrap();

        assert!(env.contains("NOUS_API_KEY=nous-test\n"));
        assert!(env.contains("OPENROUTER_API_KEY=or-test\n"));
        assert!(env.contains("AWS_BEARER_TOKEN_BEDROCK=bedrock-test\n"));
        assert!(env.contains("OPENAI_API_KEY=bedrock-test\n"));
        assert!(env.contains("OPENAI_BASE_URL=https://bedrock-mantle.ap-southeast-1.api.aws/v1\n"));
        assert!(env.contains("AWS_REGION=ap-southeast-1\n"));
        assert!(env.contains("TELEGRAM_BOT_TOKEN=123456:abc\n"));
        assert!(env.contains("TELEGRAM_ALLOWED_USERS=111,222\n"));
        assert!(env.contains("TELEGRAM_HOME_CHANNEL=111\n"));
        assert!(env.contains("TERMINAL_ENV=local\n"));
    }

    #[test]
    fn bedrock_config_sets_nova_pro_in_singapore() {
        let config = build_bedrock_config_content(
            DEFAULT_BEDROCK_REGION,
            DEFAULT_BEDROCK_MODEL,
            TerminalBackend::Local,
        );
        assert!(config.contains("default: 'amazon.nova-pro-v1:0'"));
        assert!(config.contains("provider: custom"));
        assert!(config.contains("base_url: 'https://bedrock-mantle.ap-southeast-1.api.aws/v1'"));
        assert!(config.contains("region: 'ap-southeast-1'"));
        assert!(config.ends_with("terminal:\n  backend: local\n"));
    }

    #[test]
    fn auto_provider_prefers_openrouter_when_key_present() {
        let settings = test_settings("", Some("sk-or-test"), Some("bedrock-test"));
        assert_eq!(
            resolve_ai_provider(&settings).unwrap(),
            AiProvider::OpenRouter
        );

        let settings = test_settings("", None, Some("bedrock-test"));
        assert_eq!(resolve_ai_provider(&settings).unwrap(), AiProvider::Bedrock);
    }

    #[test]
    fn explicit_provider_requires_its_key() {
        let settings = test_settings("openrouter", None, Some("bedrock-test"));
        assert!(resolve_ai_provider(&settings).is_err());

        let settings = test_settings("bedrock", Some("sk-or-test"), None);
        assert!(resolve_ai_provider(&settings).is_err());
    }

    #[test]
    fn openrouter_config_and_env_use_gpt_6_sol() {
        let settings = test_settings("", Some("sk-or-test"), None);
        let env = build_env_content(&settings).unwrap();
        assert!(env.contains("OPENROUTER_API_KEY=sk-or-test\n"));
        assert!(!env.contains("AWS_BEARER_TOKEN_BEDROCK"));

        let config =
            build_openrouter_config_content(DEFAULT_OPENROUTER_MODEL, TerminalBackend::Local);
        assert!(config.contains("default: 'openai/gpt-6-sol'"));
        assert!(config.contains("provider: openrouter"));
        assert!(config.contains("base_url: 'https://openrouter.ai/api/v1'"));
    }

    #[test]
    fn docker_backend_sandboxes_config_and_env() {
        let mut settings = test_settings("", Some("sk-or-test"), None);
        settings.terminal_backend = TerminalBackend::Docker;
        let (config, summary) = build_config_content(&settings).unwrap();
        assert!(config.ends_with("terminal:\n  backend: docker\n"));
        assert_eq!(summary, "OpenRouter openai/gpt-6-sol");
        let env = build_env_content(&settings).unwrap();
        assert!(env.contains("TERMINAL_ENV=docker\n"));
        assert!(!env.contains("TERMINAL_ENV=local"));
    }

    #[test]
    fn terminal_backend_accepts_only_local_or_docker() {
        assert_eq!(TerminalBackend::parse("").unwrap(), TerminalBackend::Local);
        assert_eq!(
            TerminalBackend::parse(" Docker ").unwrap(),
            TerminalBackend::Docker
        );
        assert!(TerminalBackend::parse("ssh").is_err());
    }

    #[test]
    fn opencode_config_uses_openrouter_model() {
        let config = build_opencode_config_content(DEFAULT_OPENROUTER_MODEL);
        let value: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(value["model"], "openrouter/openai/gpt-6-sol");
        assert_eq!(
            value["providers"]["openrouter"]["env"][0],
            "OPENROUTER_API_KEY"
        );
    }

    #[test]
    fn hermes_and_opencode_share_the_same_openrouter_model() {
        let mut settings = test_settings("", Some("sk-or-test"), None);
        settings.openrouter_model = "openai/gpt-6.1-sol".to_string();
        let model = normalized_openrouter_model(&settings);

        let hermes = build_openrouter_config_content(&model, TerminalBackend::Local);
        let opencode: serde_json::Value =
            serde_json::from_str(&build_opencode_config_content(&model)).unwrap();

        assert!(hermes.contains("provider: openrouter"));
        assert!(hermes.contains("default: 'openai/gpt-6.1-sol'"));
        assert_eq!(opencode["model"], "openrouter/openai/gpt-6.1-sol");
    }

    #[test]
    fn validates_telegram_bot_tokens() {
        assert!(validate_telegram_bot_token("123456789:ABCdefGHIjklMNOpqrSTUvwxYZ").is_ok());
        assert!(validate_telegram_bot_token("123456789:abc-def_123").is_ok());
        assert!(validate_telegram_bot_token("").is_err());
        assert!(validate_telegram_bot_token("not-a-token").is_err());
        assert!(validate_telegram_bot_token("abc:secret").is_err());
        assert!(validate_telegram_bot_token("123:bad secret").is_err());
    }
}
