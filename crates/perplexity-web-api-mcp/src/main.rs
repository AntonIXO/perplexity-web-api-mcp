//! MCP server exposing Perplexity AI tools for search, research, and reasoning.

mod server;

use perplexity_web_api::{
    AuthCookies, Client, ComputerModel, ModelPreference, ReasonModel, SearchModel,
};
use rmcp::{ServiceExt, transport::stdio};
use std::{env, env::VarError, time::Duration};
use tracing_subscriber::fmt;

use crate::server::PerplexityServer;

#[cfg(feature = "streamable-http")]
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    match signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        Err(err) => {
            tracing::warn!("Failed to register SIGTERM handler: {}", err);
            if let Err(ctrl_c_err) = tokio::signal::ctrl_c().await {
                tracing::warn!("Failed to listen for SIGINT: {}", ctrl_c_err);
            }
        }
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::warn!("Failed to listen for shutdown signal: {}", err);
    }
}

/// Reads an optional string environment variable, returning `None` if not present.
fn optional_env(name: &str) -> Result<Option<String>, std::io::Error> {
    match env::var(name) {
        Ok(value) => {
            let trimmed = value.trim().to_owned();
            if trimmed.is_empty() { Ok(None) } else { Ok(Some(trimmed)) }
        }
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(std::io::Error::other(format!(
            "Environment variable {name} must be valid UTF-8"
        ))),
    }
}

/// Reads an optional model preference from environment, accepting either a
/// known model name (validated against the typed enum) or a `raw:<preference>`
/// escape hatch that passes an arbitrary preference string straight through to
/// the Perplexity API. The escape hatch lets brand-new Perplexity models be
/// used without a recompile; the typed path stays validated.
fn optional_model_pref_env<T>(name: &str) -> Result<Option<ModelPreference>, std::io::Error>
where
    T: std::str::FromStr + Into<ModelPreference>,
    T::Err: std::fmt::Display,
{
    let Some(value) = optional_env(name)? else {
        return Ok(None);
    };

    if let Some(raw) = value.strip_prefix("raw:") {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(std::io::Error::other(format!(
                "Invalid environment variable {name}: 'raw:' prefix requires a preference string"
            )));
        }
        return Ok(Some(ModelPreference::from_raw(raw.to_owned())));
    }

    let model = value.parse::<T>().map_err(|e| {
        std::io::Error::other(format!(
            "Invalid environment variable {name}: {e}. \
             To use a model not in the validated list, pass it as 'raw:<preference>'."
        ))
    })?;
    Ok(Some(model.into()))
}

/// Reads an optional duration (in whole seconds) from an environment variable.
fn optional_duration_env(name: &str) -> Result<Option<Duration>, std::io::Error> {
    let Some(value) = optional_env(name)? else {
        return Ok(None);
    };
    let secs: u64 = value.parse().map_err(|_| {
        std::io::Error::other(format!(
            "Invalid environment variable {name}: expected a whole number of seconds"
        ))
    })?;
    Ok(Some(Duration::from_secs(secs)))
}

/// Reads an optional boolean environment variable, returning `default` if not present.
fn optional_bool_env(name: &str, default: bool) -> Result<bool, std::io::Error> {
    optional_env(name)?.as_deref().map_or(Ok(default), |value| parse_bool_env(name, value))
}

fn parse_bool_env(name: &str, value: &str) -> Result<bool, std::io::Error> {
    if value.eq_ignore_ascii_case("true") {
        Ok(true)
    } else if value.eq_ignore_ascii_case("false") {
        Ok(false)
    } else {
        Err(std::io::Error::other(format!(
            "Invalid environment variable {name}: expected true/false"
        )))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (logs to stderr to not interfere with stdio transport)
    fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let session_token = optional_env("PERPLEXITY_SESSION_TOKEN")?;
    let tokenless = session_token.is_none();
    let incognito = optional_bool_env("PERPLEXITY_INCOGNITO", true)?;

    let (default_ask_model, default_reason_model, default_computer_model) = if tokenless {
        // In tokenless mode, model overrides are not supported.
        // Use the same trim-and-check-empty semantics as optional_env
        // so that setting an empty/whitespace-only value is treated as "unset".
        // Note: since PR #14 the CSRF token is fetched dynamically, so only
        // PERPLEXITY_SESSION_TOKEN is required to enable model configuration.
        for name in
            ["PERPLEXITY_ASK_MODEL", "PERPLEXITY_REASON_MODEL", "PERPLEXITY_COMPUTER_MODEL"]
        {
            if optional_env(name)?.is_some() {
                return Err(std::io::Error::other(format!(
                    "{name} cannot be used without authentication.\n\n\
                     To use model configuration, provide:\n\
                       PERPLEXITY_SESSION_TOKEN  - Perplexity session token",
                ))
                .into());
            }
        }
        (Some(ModelPreference::from(SearchModel::Turbo)), None, None)
    } else {
        let ask = optional_model_pref_env::<SearchModel>("PERPLEXITY_ASK_MODEL")?
            .unwrap_or_else(|| SearchModel::ProAuto.into());
        let reason = optional_model_pref_env::<ReasonModel>("PERPLEXITY_REASON_MODEL")?;
        let computer = optional_model_pref_env::<ComputerModel>("PERPLEXITY_COMPUTER_MODEL")?;
        (Some(ask), reason, computer)
    };

    if tokenless {
        tracing::info!(
            "Starting Perplexity MCP server in tokenless mode (only perplexity_search and \
             perplexity_ask with turbo model are available)"
        );
    } else {
        tracing::info!("Starting Perplexity MCP server");
    }
    tracing::info!(
        "Perplexity request incognito mode is {}",
        if incognito { "enabled" } else { "disabled" }
    );

    let mut builder = Client::builder();
    if let Some(session) = session_token {
        builder = builder.cookies(AuthCookies::new(session));
    }
    // Optional timeout overrides. Long-running modes (Deep Research, Computer,
    // Document Review) use the larger `long_timeout` budget so they aren't
    // aborted mid-run; the default is 10 minutes.
    if let Some(t) = optional_duration_env("PERPLEXITY_TIMEOUT_SECS")? {
        builder = builder.timeout(t);
    }
    if let Some(t) = optional_duration_env("PERPLEXITY_LONG_TIMEOUT_SECS")? {
        builder = builder.long_timeout(t);
    }

    let client = builder.build().await.map_err(|e| {
        tracing::error!("Failed to create Perplexity client: {}", e);
        e
    })?;

    tracing::info!("Perplexity client initialized");

    // How often to emit `notifications/progress` heartbeats during long-running
    // tool calls (only sent when the client supplied a `progressToken`).
    // Spec-compliant clients with `resetTimeoutOnProgress` enabled can then
    // extend their own per-request timeout on each heartbeat instead of
    // needing an ever-larger static `timeout` in their MCP client config.
    // Default 10s is comfortably inside typical client timeouts (60-180s)
    // while staying cheap.
    let progress_interval = optional_duration_env("PERPLEXITY_PROGRESS_INTERVAL_SECS")?
        .unwrap_or(Duration::from_secs(10));

    let server = PerplexityServer::new(
        client,
        default_ask_model,
        default_reason_model,
        default_computer_model,
        tokenless,
        incognito,
        progress_interval,
    );

    let transport = optional_env("MCP_TRANSPORT")?.unwrap_or_else(|| "stdio".to_owned());

    match transport.as_str() {
        "stdio" => {
            let service = server.serve(stdio()).await.inspect_err(|e| {
                tracing::error!("Server error: {:?}", e);
            })?;

            tracing::info!("MCP server running on stdio");

            tokio::select! {
                result = service.waiting() => {
                    result?;
                }
                _ = shutdown_signal() => {
                    tracing::info!("Shutdown signal received, stopping MCP server");
                }
            }
        }
        #[cfg(feature = "streamable-http")]
        "streamable-http" => {
            let host = optional_env("MCP_HOST")?.unwrap_or_else(|| "0.0.0.0".to_owned());
            let port = optional_env("MCP_PORT")?.unwrap_or_else(|| "8080".to_owned());
            let addr = format!("{host}:{port}");

            let http_service = StreamableHttpService::new(
                move || Ok(server.clone()),
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let app = axum::Router::new().nest_service("/mcp", http_service);
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            tracing::info!("MCP server listening on http://{addr}/mcp");
            axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
        }
        #[cfg(not(feature = "streamable-http"))]
        "streamable-http" => {
            return Err(std::io::Error::other(
                "MCP_TRANSPORT=streamable-http requires building with the `streamable-http` cargo feature",
            )
            .into());
        }
        other => {
            #[cfg(feature = "streamable-http")]
            let valid_values = "'stdio', 'streamable-http'";
            #[cfg(not(feature = "streamable-http"))]
            let valid_values = "'stdio'";
            return Err(std::io::Error::other(format!(
                "Unknown MCP_TRANSPORT value: '{other}'. Valid values: {valid_values}"
            ))
            .into());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_bool_env;

    #[test]
    fn parses_truthy_values() {
        for value in ["true", "TRUE"] {
            assert!(parse_bool_env("TEST_BOOL", value).unwrap());
        }
    }

    #[test]
    fn parses_falsy_values() {
        for value in ["false", "FALSE"] {
            assert!(!parse_bool_env("TEST_BOOL", value).unwrap());
        }
    }

    #[test]
    fn uses_default_when_value_is_missing() {
        assert!(optional_bool_env_value(None, true).unwrap());
        assert!(!optional_bool_env_value(None, false).unwrap());
    }

    #[test]
    fn rejects_invalid_values() {
        let error = parse_bool_env("TEST_BOOL", "maybe").unwrap_err();
        assert!(error.to_string().contains("TEST_BOOL"));
    }

    fn optional_bool_env_value(
        value: Option<&str>,
        default: bool,
    ) -> Result<bool, std::io::Error> {
        value.map_or(Ok(default), |value| parse_bool_env("TEST_BOOL", value))
    }
}
