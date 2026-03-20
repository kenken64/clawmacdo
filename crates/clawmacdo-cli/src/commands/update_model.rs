use anyhow::{bail, Result};
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
use std::path::PathBuf;

const VALID_MODELS: &[&str] = &["anthropic", "openai", "gemini", "byteplus"];

fn has_value(s: &str) -> bool {
    !s.trim().is_empty()
}

fn model_identifier(model: &str) -> Option<&'static str> {
    match model {
        "anthropic" => Some("anthropic/claude-opus-4-6"),
        "openai" => Some("openai/gpt-5-mini"),
        "gemini" => Some("google/gemini-2.5-flash"),
        "byteplus" => Some("byteplus/ark-code-latest"),
        _ => None,
    }
}

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

pub struct UpdateModelParams {
    pub instance: String,
    pub primary_model: String,
    pub failover_1: String,
    pub failover_2: String,
    pub anthropic_key: String,
    pub openai_key: String,
    pub gemini_key: String,
    pub byteplus_ark_api_key: String,
}

pub async fn run(params: UpdateModelParams) -> Result<()> {
    // Validate primary model
    if !VALID_MODELS.contains(&params.primary_model.as_str()) {
        bail!(
            "Invalid primary model '{}'. Must be one of: {}",
            params.primary_model,
            VALID_MODELS.join(", ")
        );
    }

    // Validate failovers
    for (label, fo) in [
        ("failover-1", &params.failover_1),
        ("failover-2", &params.failover_2),
    ] {
        if has_value(fo) && !VALID_MODELS.contains(&fo.as_str()) {
            bail!(
                "Invalid {label} model '{fo}'. Must be one of: {}",
                VALID_MODELS.join(", ")
            );
        }
    }

    let (ip, key, _provider) = find_deploy_record(&params.instance)?;
    let home = config::OPENCLAW_HOME;

    println!("Updating AI model on {ip}...");

    // Step 1: Update API keys in .env
    println!("[1/4] Updating API keys in .env...");
    let key_updates: Vec<(&str, &str)> = vec![
        ("ANTHROPIC_API_KEY", &params.anthropic_key),
        ("OPENAI_API_KEY", &params.openai_key),
        ("GEMINI_API_KEY", &params.gemini_key),
        ("BYTEPLUS_API_KEY", &params.byteplus_ark_api_key),
    ];

    let mut env_cmds = Vec::new();
    let mut any_key_updated = false;
    for (env_var, value) in &key_updates {
        if has_value(value) {
            env_cmds.push(format!(
                "if grep -q '^{env_var}=' {home}/.openclaw/.env 2>/dev/null; then \
                   sed -i 's|^{env_var}=.*|{env_var}={value}|' {home}/.openclaw/.env; \
                 else \
                   echo '{env_var}={value}' >> {home}/.openclaw/.env; \
                 fi"
            ));
            any_key_updated = true;
        }
    }

    if any_key_updated {
        env_cmds.push(format!("chmod 600 {home}/.openclaw/.env"));
        let env_cmd = env_cmds.join(" && ");
        ssh_as_openclaw_async(&ip, &key, &env_cmd).await?;
        println!("  API keys updated.");
    } else {
        println!("  No API keys provided — using existing keys on instance.");
    }

    // Warn if primary model key is not provided
    let primary_key_provided = match params.primary_model.as_str() {
        "anthropic" => has_value(&params.anthropic_key),
        "openai" => has_value(&params.openai_key),
        "gemini" => has_value(&params.gemini_key),
        "byteplus" => has_value(&params.byteplus_ark_api_key),
        _ => false,
    };
    if !primary_key_provided {
        println!(
            "  Warning: No API key provided for '{}'. The instance must already have it in .env.",
            params.primary_model
        );
    }

    // Step 2: Update BytePlus provider config in openclaw.json (if byteplus is involved)
    println!("[2/4] Updating provider configuration...");
    let needs_byteplus = params.primary_model == "byteplus"
        || (has_value(&params.failover_1) && params.failover_1 == "byteplus")
        || (has_value(&params.failover_2) && params.failover_2 == "byteplus");

    if needs_byteplus && has_value(&params.byteplus_ark_api_key) {
        let bp_cmd = format!(
            "node -e 'const fs=require(\"fs\");\
const p=\"{home}/.openclaw/openclaw.json\";\
let cfg={{}};\
try {{ cfg=JSON.parse(fs.readFileSync(p,\"utf8\")); }} catch(e) {{}}\
cfg.models=cfg.models||{{}};\
cfg.models.providers=cfg.models.providers||{{}};\
cfg.models.providers.byteplus={{\
\"baseUrl\":\"https://ark.ap-southeast.bytepluses.com/api/coding/v3\",\
\"apiKey\":\"'\"$BYTEPLUS_API_KEY\"'\",\
\"api\":\"openai-completions\",\
\"models\":[{{\"id\":\"ark-code-latest\",\"name\":\"ark-code-latest\"}}]\
}};\
cfg.auth=cfg.auth||{{}};\
cfg.auth.profiles=cfg.auth.profiles||{{}};\
cfg.auth.profiles[\"byteplus:default\"]={{\"provider\":\"byteplus\",\"mode\":\"api_key\"}};\
fs.writeFileSync(p,JSON.stringify(cfg,null,2)+\"\\n\");' && echo ok"
        );
        let out = ssh_as_openclaw_async(&ip, &key, &bp_cmd).await?;
        if !out.trim().is_empty() {
            println!("  {}", out.trim());
        }
    } else if needs_byteplus {
        println!("  Warning: BytePlus model selected but no --byteplus-ark-api-key provided.");
        println!("  The instance must already have BytePlus provider configured in openclaw.json.");
    } else {
        println!("  No provider config changes needed.");
    }

    // Step 3: Set primary model and failovers
    println!("[3/4] Setting primary model and failovers...");
    let uid = "$(id -u)";
    let primary_id = model_identifier(&params.primary_model).unwrap_or("anthropic/claude-opus-4-6");
    let mut model_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:$PATH\" \
         XDG_RUNTIME_DIR=/run/user/{uid} \
         DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus; \
         openclaw models set {primary_id} 2>&1 || true;"
    );

    for fo in [&params.failover_1, &params.failover_2] {
        if has_value(fo) {
            if let Some(fo_id) = model_identifier(fo) {
                model_cmd.push_str(&format!(
                    " openclaw models fallbacks add {fo_id} 2>&1 || true;"
                ));
            }
        }
    }
    model_cmd.push_str(" echo ok");

    let model_out = ssh_as_openclaw_async(&ip, &key, &model_cmd).await?;
    if !model_out.trim().is_empty() {
        println!("  {}", model_out.trim());
    }

    // Step 4: Restart gateway service
    println!("[4/4] Restarting gateway service...");
    let restart_cmd =
        "export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         (systemctl --user daemon-reload 2>/dev/null || true) && \
         (systemctl --user restart openclaw-gateway.service 2>/dev/null || \
          systemctl --user start openclaw-gateway.service 2>/dev/null || true) && \
         sleep 2 && \
         echo -n 'gateway: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true)";
    let restart_out = ssh_as_openclaw_async(&ip, &key, restart_cmd).await?;
    println!("  {}", restart_out.trim());

    // Summary
    println!();
    println!("AI model updated on {ip}:");
    println!("  Primary: {} ({})", params.primary_model, primary_id);
    if has_value(&params.failover_1) {
        if let Some(fo_id) = model_identifier(&params.failover_1) {
            println!("  Failover 1: {} ({})", params.failover_1, fo_id);
        }
    }
    if has_value(&params.failover_2) {
        if let Some(fo_id) = model_identifier(&params.failover_2) {
            println!("  Failover 2: {} ({})", params.failover_2, fo_id);
        }
    }

    Ok(())
}
