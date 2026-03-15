use anyhow::{bail, Result};
use chrono::{TimeZone, Utc};
use reqwest::Client;

/// List ARK inference endpoints for the given BytePlus account.
pub async fn list_endpoints(access_key: &str, secret_key: &str) -> Result<()> {
    if access_key.is_empty() || secret_key.is_empty() {
        bail!("BytePlus access key and secret key are required.\nSet BYTEPLUS_ACCESS_KEY and BYTEPLUS_SECRET_KEY environment variables or pass --access-key and --secret-key.");
    }

    let client =
        clawmacdo_cloud::byteplus::BytePlusClient::new(access_key, secret_key, "ap-southeast-1")?;

    println!("Fetching ARK endpoints...\n");
    let endpoints = client.list_endpoints().await?;

    if endpoints.is_empty() {
        println!("No endpoints found. Create one in the BytePlus ARK console first.");
        return Ok(());
    }

    println!(
        "{:<30} {:<20} {:<10} MODEL",
        "ENDPOINT ID", "NAME", "STATUS"
    );
    println!("{}", "-".repeat(90));

    for ep in &endpoints {
        let id = ep
            .get("Id")
            .or_else(|| ep.get("EndpointId"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let name = ep
            .get("Name")
            .or_else(|| ep.get("EndpointName"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let status = ep.get("Status").and_then(|v| v.as_str()).unwrap_or("-");
        let model = ep
            .get("ModelReference")
            .and_then(|v| v.get("ModelId").or_else(|| v.get("FoundationModel")))
            .and_then(|v| v.as_str())
            .or_else(|| ep.get("Model").and_then(|v| v.as_str()))
            .unwrap_or("-");

        println!("{id:<30} {name:<20} {status:<10} {model}");
    }

    println!("\nUse --resource-ids <ENDPOINT_ID> to generate an API key for a specific endpoint.");
    Ok(())
}

/// Generate a temporary BytePlus ARK API key using access/secret key credentials.
pub async fn get_api_key(
    access_key: &str,
    secret_key: &str,
    resource_type: &str,
    resource_ids: &[String],
    duration_seconds: u64,
) -> Result<()> {
    if access_key.is_empty() || secret_key.is_empty() {
        bail!("BytePlus access key and secret key are required.\nSet BYTEPLUS_ACCESS_KEY and BYTEPLUS_SECRET_KEY environment variables or pass --access-key and --secret-key.");
    }
    if resource_ids.is_empty() {
        bail!("At least one resource ID is required (endpoint or bot ID).\nUse --list to see available endpoints.");
    }
    if duration_seconds == 0 || duration_seconds > 2_592_000 {
        bail!("Duration must be between 1 and 2592000 seconds (30 days).");
    }

    let valid_types = ["endpoint", "bot"];
    if !valid_types.contains(&resource_type) {
        bail!("Resource type must be 'endpoint' or 'bot', got '{resource_type}'.");
    }

    println!("Generating BytePlus ARK API key...");
    println!("  Resource type: {resource_type}");
    println!("  Resource IDs:  {}", resource_ids.join(", "));
    println!(
        "  Duration:      {} seconds ({:.1} days)",
        duration_seconds,
        duration_seconds as f64 / 86400.0
    );

    let client =
        clawmacdo_cloud::byteplus::BytePlusClient::new(access_key, secret_key, "ap-southeast-1")?;

    let (api_key, expired_time) = client
        .get_api_key(resource_type, resource_ids, duration_seconds)
        .await?;

    let expires_str = if expired_time > 0 {
        Utc.timestamp_opt(expired_time as i64, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| expired_time.to_string())
    } else {
        "unknown".to_string()
    };

    println!("\nARK API Key generated successfully.");
    println!("  API Key:  {api_key}");
    println!("  Expires:  {expires_str}");
    println!("\nUse this key as a Bearer token for ARK inference endpoints.");

    Ok(())
}

const ARK_BASE_URL: &str = "https://ark.ap-southeast.bytepluses.com/api/v3";

/// Send a chat completion prompt to a BytePlus ARK endpoint.
pub async fn chat(api_key: &str, endpoint_id: &str, prompt: &str) -> Result<()> {
    if api_key.is_empty() {
        bail!("ARK API key is required. Generate one with: clawmacdo ark-api-key --resource-ids <endpoint-id>");
    }
    if endpoint_id.is_empty() {
        bail!("Endpoint ID is required. Use: clawmacdo ark-api-key --list");
    }
    if prompt.is_empty() {
        bail!("Prompt cannot be empty.");
    }

    let url = format!("{ARK_BASE_URL}/chat/completions");

    let body = serde_json::json!({
        "model": endpoint_id,
        "messages": [
            {"role": "user", "content": prompt}
        ]
    });

    let client = Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    let resp_body: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        let err_msg = resp_body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        bail!("ARK API error ({status}): {err_msg}");
    }

    let content = resp_body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("<no response>");

    println!("{content}");

    // Print usage stats if available
    if let Some(usage) = resp_body.get("usage") {
        let prompt_tokens = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let completion_tokens = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        eprintln!(
            "\n[tokens: {prompt_tokens} prompt + {completion_tokens} completion = {} total]",
            prompt_tokens + completion_tokens
        );
    }

    Ok(())
}
