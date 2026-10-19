//! Local runtime discovery and explicitly requested, synthetic inference benchmarks.
//!
//! Discovery is read-only. HTTP destinations are pinned to loopback, proxies and
//! redirects are disabled. A local server can still route work elsewhere; its
//! configuration remains the operator's responsibility. Known Ollama remote
//! models are excluded and checked again before a benchmark.

use crate::model::{BenchmarkResult, LlmState, ModelInfo, RuntimeStatus};
use anyhow::{Result, anyhow, bail};
use reqwest::{Client, Response, Url};
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_STREAM: usize = 1024 * 1024;
const MAX_LINE: usize = 64 * 1024;
const MAX_MODELS: usize = 1024;
const SYNTHETIC_PROMPT: &str =
    "Descrivi in italiano, in tre brevi frasi, come organizzare una scrivania ordinata.";

#[derive(Clone, Debug)]
pub struct Endpoints {
    pub ollama: String,
    pub lm_studio: String,
    pub llama_cpp: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            ollama: "http://127.0.0.1:11434".into(),
            lm_studio: "http://127.0.0.1:1234".into(),
            llama_cpp: "http://127.0.0.1:8080".into(),
        }
    }
}

/// Accept a bare HTTP loopback origin only. `localhost` is pinned to 127.0.0.1,
/// so neither DNS nor the hosts file can redirect requests to a remote machine.
pub fn validate_endpoint(endpoint: &str) -> Result<()> {
    local_url(endpoint).map(|_| ())
}

pub(crate) fn local_url(endpoint: &str) -> Result<Url> {
    if endpoint.trim() != endpoint || endpoint.chars().any(char::is_control) {
        bail!("Endpoint non valido: usare un'origine HTTP loopback senza spazi");
    }
    // Check the original syntax before URL canonicalization can collapse a
    // path such as /a/../, an empty userinfo, or backslash separators.
    let raw_authority = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| anyhow!("Endpoint HTTP non valido"))?;
    if raw_authority.contains(['@', '\\'])
        || raw_authority
            .split_once('/')
            .is_some_and(|(_, path)| !path.is_empty())
    {
        bail!("L'endpoint deve contenere soltanto un'origine HTTP locale");
    }
    let mut url = Url::parse(endpoint).map_err(|_| anyhow!("Endpoint HTTP non valido"))?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("Usare solo http://IP-loopback:porta, senza credenziali, percorsi o parametri");
    }
    match url.host_str() {
        Some("localhost") => {
            url.set_host(Some("127.0.0.1"))
                .map_err(|_| anyhow!("Endpoint locale non valido"))?;
        }
        Some(host)
            if host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()) => {}
        _ => bail!("Sono consentiti soltanto localhost o indirizzi IP loopback"),
    }
    if url.port() == Some(0) {
        bail!("La porta deve essere compresa tra 1 e 65535");
    }
    Ok(url)
}

pub(crate) fn canonical_runtime(runtime: &str) -> Result<&'static str> {
    match runtime
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
        .as_str()
    {
        "ollama" => Ok("Ollama"),
        "lmstudio" => Ok("LM Studio"),
        "llama.cpp" | "llamacpp" => Ok("llama.cpp"),
        _ => bail!("Runtime non supportato: scegliere Ollama, LM Studio o llama.cpp"),
    }
}

fn authorization(token: Option<String>) -> Result<reqwest::header::HeaderMap> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        if token.len() > 8192 || token.chars().any(char::is_control) {
            bail!("Token del runtime non valido; controllare la variabile d'ambiente");
        }
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("Token del runtime non valido"))?;
        value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    Ok(headers)
}

pub(crate) fn environment_token(variable: &'static str) -> Result<Option<String>> {
    match std::env::var(variable) {
        Ok(token) => Ok((!token.is_empty()).then_some(token)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("La variabile {variable} deve contenere un token Unicode valido")
        }
    }
}

pub(crate) fn runtime_token(runtime: &str) -> Result<Option<String>> {
    let variable = match canonical_runtime(runtime)? {
        "Ollama" => "RUST_CPU_OLLAMA_TOKEN",
        "LM Studio" => "RUST_CPU_LM_STUDIO_TOKEN",
        _ => "RUST_CPU_LLAMA_CPP_TOKEN",
    };
    environment_token(variable)
}

pub(crate) fn client_with_token(probe: bool, token: Option<String>) -> Result<Client> {
    build_client(probe, authorization(token)?)
}

pub(crate) fn client_for(probe: bool, runtime: &str) -> Result<Client> {
    client_with_token(probe, runtime_token(runtime)?)
}

fn build_client(probe: bool, headers: reqwest::header::HeaderMap) -> Result<Client> {
    Client::builder()
        .default_headers(headers)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .read_timeout(Duration::from_secs(if probe { 1 } else { 30 }))
        .timeout(Duration::from_secs(if probe { 1 } else { 120 }))
        .user_agent("rust-cpu/local-observability")
        .build()
        .map_err(|_| anyhow!("Impossibile inizializzare il client HTTP locale"))
}

fn executor() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| anyhow!("Impossibile inizializzare il runtime HTTP"))
}

#[derive(Debug)]
pub(crate) enum Issue {
    Offline,
    Timeout,
    Http(u16),
    Malformed,
    TooLarge,
    Transport,
    Cancelled,
}
impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Offline => "Server locale non raggiungibile",
            Self::Timeout => "Timeout del server locale",
            Self::Http(code) => return write!(f, "Il server locale ha risposto HTTP {code}"),
            Self::Malformed => "Risposta API non valida o incompleta",
            Self::TooLarge => "Risposta API oltre il limite di sicurezza",
            Self::Transport => "Errore nel trasporto HTTP locale",
            Self::Cancelled => "Benchmark annullato",
        };
        f.write_str(message)
    }
}
impl std::error::Error for Issue {}

fn transport_error(error: reqwest::Error) -> Issue {
    if error.is_timeout() {
        Issue::Timeout
    } else if error.is_connect() {
        Issue::Offline
    } else {
        Issue::Transport
    }
}

pub(crate) async fn cancellable<T>(
    future: impl Future<Output = reqwest::Result<T>>,
    cancel: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<T, Issue> {
    let mut future = std::pin::pin!(future);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Issue::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Issue::Timeout);
        }
        if let Ok(result) = tokio::time::timeout(Duration::from_millis(50), &mut future).await {
            if cancel.load(Ordering::Relaxed) {
                return Err(Issue::Cancelled);
            }
            return result.map_err(transport_error);
        }
    }
}

fn route(base: &Url, path: &str) -> Url {
    let mut url = base.clone();
    url.set_path(path);
    url
}

async fn response_json(
    mut response: Response,
    cancel: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<Value, Issue> {
    let status = response.status();
    if !status.is_success() {
        return Err(Issue::Http(status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BODY as u64)
    {
        return Err(Issue::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = cancellable(response.chunk(), cancel, deadline).await? {
        if body.len().saturating_add(chunk.len()) > MAX_BODY {
            return Err(Issue::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body).map_err(|_| Issue::Malformed)?;
    if !value.is_object() {
        return Err(Issue::Malformed);
    }
    Ok(value)
}

pub(crate) async fn get_json(
    client: &Client,
    base: &Url,
    path: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<Value, Issue> {
    let response = cancellable(client.get(route(base, path)).send(), cancel, deadline).await?;
    response_json(response, cancel, deadline).await
}

fn safe_text(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(256)
        .collect()
}

fn model_array<'a>(value: &'a Value, field: &str) -> std::result::Result<&'a [Value], Issue> {
    let list = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or(Issue::Malformed)?;
    if list.len() > MAX_MODELS {
        return Err(Issue::TooLarge);
    }
    Ok(list)
}

fn model_id(value: &Value) -> Option<&str> {
    value
        .get("name")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn remote_model(value: &Value) -> bool {
    ["remote_host", "remote_model"].iter().any(|field| {
        value
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    }) || model_id(value).is_some_and(|id| id.to_ascii_lowercase().contains("cloud"))
}

/// Tags do not advertise complete capabilities. Exclude only known encoder
/// families and the exact nomic embedding model, rather than guessing from any
/// occurrence of "embed" in an otherwise valid generative model's name.
fn embedding_only(value: &Value) -> bool {
    let encoder_family = |family: &str| {
        family.eq_ignore_ascii_case("bert") || family.eq_ignore_ascii_case("nomic-bert")
    };
    value
        .pointer("/details/family")
        .and_then(Value::as_str)
        .is_some_and(encoder_family)
        || value
            .pointer("/details/families")
            .and_then(Value::as_array)
            .is_some_and(|families| {
                families
                    .iter()
                    .filter_map(Value::as_str)
                    .any(encoder_family)
            })
        || model_id(value).is_some_and(|id| {
            id.rsplit('/')
                .next()
                .unwrap_or(id)
                .split(':')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("nomic-embed-text"))
        })
}

pub(crate) async fn ensure_local_ollama_model(
    client: &Client,
    base: &Url,
    model: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<()> {
    if model.to_ascii_lowercase().contains("cloud") {
        bail!("Sono consentiti soltanto modelli Ollama locali");
    }
    let tags = get_json(client, base, "/api/tags", cancel, deadline).await?;
    let tagged = if model
        .rsplit('/')
        .next()
        .is_some_and(|name| !name.contains(':'))
    {
        format!("{model}:latest")
    } else {
        model.to_string()
    };
    let local = model_array(&tags, "models")?
        .iter()
        .find(|row| model_id(row).is_some_and(|name| name == model || name == tagged));
    if local.is_some_and(embedding_only) {
        bail!("Modello embedding non generativo: scegliere un modello di generazione testuale");
    }
    if !local.is_some_and(|row| {
        !remote_model(row)
            && row
                .get("size")
                .and_then(Value::as_u64)
                .is_some_and(|size| size > 0)
    }) {
        bail!(
            "Il modello deve risultare installato localmente; modelli remoti o non verificabili esclusi"
        );
    }
    Ok(())
}

fn ollama_models(
    tags: &Value,
    running: Option<&Value>,
) -> std::result::Result<Vec<ModelInfo>, Issue> {
    let list = model_array(tags, "models")?;
    let loaded = running.map(|v| model_array(v, "models")).transpose()?;
    let mut result = Vec::new();
    for value in list {
        if remote_model(value) || embedding_only(value) {
            continue;
        }
        let Some(id) = model_id(value) else {
            return Err(Issue::Malformed);
        };
        let active = loaded.and_then(|rows| rows.iter().find(|row| model_id(row) == Some(id)));
        result.push(ModelInfo {
            id: safe_text(id),
            size_bytes: value.get("size").and_then(Value::as_u64),
            vram_bytes: active
                .and_then(|v| v.get("size_vram"))
                .and_then(Value::as_u64),
            context_length: active
                .and_then(|v| v.get("context_length"))
                .and_then(Value::as_u64),
            quantization: value
                .pointer("/details/quantization_level")
                .and_then(Value::as_str)
                .map(safe_text),
            loaded: loaded.map(|_| active.is_some()),
        });
    }
    Ok(result)
}

fn studio_models(value: &Value, native: bool) -> std::result::Result<Vec<ModelInfo>, Issue> {
    let mut result = Vec::new();
    for row in model_array(value, if native { "models" } else { "data" })? {
        if native
            && row
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t != "llm")
        {
            continue;
        }
        let instances = row.get("loaded_instances").and_then(Value::as_array);
        let id = instances
            .and_then(|v| v.first())
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .or_else(|| {
                row.get(if native { "key" } else { "id" })
                    .and_then(Value::as_str)
            })
            .filter(|id| !id.is_empty())
            .ok_or(Issue::Malformed)?;
        result.push(ModelInfo {
            id: safe_text(id),
            size_bytes: row.get("size_bytes").and_then(Value::as_u64),
            context_length: instances
                .and_then(|v| v.first())
                .and_then(|v| v.pointer("/config/context_length"))
                .and_then(Value::as_u64)
                .or_else(|| row.get("max_context_length").and_then(Value::as_u64)),
            quantization: row
                .pointer("/quantization/name")
                .and_then(Value::as_str)
                .map(safe_text),
            loaded: instances.map(|v| !v.is_empty()),
            ..Default::default()
        });
    }
    Ok(result)
}

fn fail_status(runtime: &mut RuntimeStatus, issue: Issue) {
    runtime.status = match issue {
        Issue::Offline | Issue::Timeout => "offline",
        Issue::Http(401 | 403) => "auth",
        Issue::Http(503) => "loading",
        _ => "error",
    }
    .into();
    runtime.message = match issue {
        Issue::Http(401 | 403) => "Autenticazione richiesta; nessuna chiave inviata".into(),
        Issue::Http(503) => "Server presente ma non pronto (HTTP 503)".into(),
        _ => issue.to_string(),
    };
}

fn probe(name: &str, endpoint: &str) -> RuntimeStatus {
    let mut status = RuntimeStatus {
        name: name.into(),
        endpoint: safe_text(endpoint),
        ..Default::default()
    };
    let base = match local_url(endpoint) {
        Ok(url) => url,
        Err(_) => {
            status.status = "invalid".into();
            status.endpoint = "endpoint non valido".into();
            status.message = "Configurare un'origine HTTP loopback".into();
            return status;
        }
    };
    let result = (|| -> Result<()> {
        let executor = executor()?;
        let client = client_for(true, name)?;
        executor
            .block_on(async {
                let cancel = AtomicBool::new(false);
                let next_deadline = || Instant::now() + Duration::from_secs(2);
                match name {
                    "Ollama" => {
                        let tags =
                            get_json(&client, &base, "/api/tags", &cancel, next_deadline()).await?;
                        model_array(&tags, "models")?;
                        let running = get_json(&client, &base, "/api/ps", &cancel, next_deadline())
                            .await
                            .and_then(|value| {
                                model_array(&value, "models")?;
                                Ok(value)
                            });
                        status.models = ollama_models(&tags, running.as_ref().ok())?;
                        status.status = "online".into();
                        let excluded = model_array(&tags, "models")?
                            .iter()
                            .filter(|v| remote_model(v))
                            .count();
                        let embeddings = model_array(&tags, "models")?.iter()
                            .filter(|value| !remote_model(value) && embedding_only(value)).count();
                        status.message = format!(
                            "{} modelli locali; {} remoti / {} embedding esclusi",
                            status.models.len(), excluded, embeddings
                        );
                        if let Err(issue) = running {
                            status
                                .message
                                .push_str(&format!("; modelli caricati n/d ({issue})"));
                        }
                    }
                    "LM Studio" => {
                        let (models, native) = match get_json(
                            &client,
                            &base,
                            "/api/v1/models",
                            &cancel,
                            next_deadline(),
                        )
                        .await
                        {
                            Ok(models) => (models, true),
                            Err(Issue::Http(404 | 405)) => (
                                get_json(&client, &base, "/v1/models", &cancel, next_deadline())
                                    .await?,
                                false,
                            ),
                            Err(issue) => return Err(issue),
                        };
                        status.models = studio_models(&models, native)?;
                        status.status = "online".into();
                        status.message = if native {
                            "API nativa v1; tipo llm dichiarato; generazione non verificata; contesto caricato o massimo"
                        } else {
                            "API OpenAI; caricamento, contesto e capacità generazione n/d"
                        }
                        .into();
                    }
                    _ => {
                        let health =
                            get_json(&client, &base, "/health", &cancel, next_deadline()).await?;
                        if health.get("status").and_then(Value::as_str) != Some("ok") {
                            return Err(Issue::Malformed);
                        }
                        let models =
                            get_json(&client, &base, "/v1/models", &cancel, next_deadline())
                                .await?;
                        status.models = studio_models(&models, false)?;
                        status.status = "online".into();
                        status.message =
                            "Health ok; identificativi API, nessuna inferenza avviata".into();
                    }
                }
                Ok::<(), Issue>(())
            })
            .map_err(anyhow::Error::from)
    })();
    if let Err(error) = result {
        if let Some(issue) = error.downcast_ref::<Issue>() {
            // Preserve the category without ever exposing response bodies or private prompts.
            status.status = match issue {
                Issue::Offline | Issue::Timeout => "offline",
                Issue::Http(401 | 403) => "auth",
                Issue::Http(503) => "loading",
                _ => "error",
            }
            .into();
            status.message = issue.to_string();
        } else {
            fail_status(&mut status, Issue::Transport);
        }
    }
    status
}

/// Probe the three configured local origins concurrently. No model is loaded,
/// downloaded or queried by discovery, including when the runtime is offline.
pub fn probe_all(endpoints: &Endpoints) -> LlmState {
    let runtimes = std::thread::scope(|scope| {
        let ollama = scope.spawn(|| probe("Ollama", &endpoints.ollama));
        let studio = scope.spawn(|| probe("LM Studio", &endpoints.lm_studio));
        let llama = scope.spawn(|| probe("llama.cpp", &endpoints.llama_cpp));
        [
            ("Ollama", ollama),
            ("LM Studio", studio),
            ("llama.cpp", llama),
        ]
        .into_iter()
        .map(|(name, task)| {
            task.join().unwrap_or_else(|_| RuntimeStatus {
                name: name.into(),
                status: "error".into(),
                message: "Errore interno del probe locale".into(),
                ..Default::default()
            })
        })
        .collect()
    });
    LlmState {
        runtimes,
        last_probe_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        benchmark: None,
        ..Default::default()
    }
}

#[derive(Default)]
struct StreamStats {
    first_token_ms: Option<f64>,
    output_tokens: Option<u64>,
    prompt_tokens: Option<u64>,
    generation_rate: Option<f64>,
    prompt_rate: Option<f64>,
    load_ms: Option<f64>,
    finished: bool,
    finish_reason: bool,
    cached_prompt: Option<u64>,
    native_timing: bool,
}

fn rate(count: Option<u64>, seconds: Option<f64>) -> Option<f64> {
    let count = count?;
    let seconds = seconds?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let value = count as f64 / seconds;
    value.is_finite().then_some(value)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
}

impl StreamStats {
    fn event(
        &mut self,
        value: &Value,
        ollama: bool,
        start: Instant,
    ) -> std::result::Result<(), Issue> {
        let failed_reason = value
            .get("done_reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| matches!(reason, "error" | "cancelled" | "canceled"))
            || value
                .get("choices")
                .and_then(Value::as_array)
                .is_some_and(|choices| {
                    choices.iter().any(|choice| {
                        choice
                            .get("finish_reason")
                            .and_then(Value::as_str)
                            .is_some_and(|reason| {
                                matches!(reason, "error" | "cancelled" | "canceled")
                            })
                    })
                });
        if !value.is_object() || value.get("error").is_some() || failed_reason {
            return Err(Issue::Malformed);
        }
        let has_token = if ollama {
            [
                "/response",
                "/thinking",
                "/message/content",
                "/message/thinking",
            ]
            .iter()
            .any(|field| {
                value
                    .pointer(field)
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            })
        } else {
            value
                .get("choices")
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter().any(|row| {
                        [
                            "/delta/content",
                            "/delta/reasoning_content",
                            "/delta/reasoning",
                            "/message/content",
                            "/message/reasoning_content",
                        ]
                        .iter()
                        .any(|path| {
                            row.pointer(path)
                                .and_then(Value::as_str)
                                .is_some_and(|s| !s.is_empty())
                        })
                    })
                })
        };
        if has_token && self.first_token_ms.is_none() {
            self.first_token_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
        }
        if ollama {
            if value.get("done").and_then(Value::as_bool) == Some(true) {
                self.finished = true;
                self.native_timing = true;
                self.output_tokens = value.get("eval_count").and_then(Value::as_u64);
                self.prompt_tokens = value.get("prompt_eval_count").and_then(Value::as_u64);
                self.cached_prompt = value
                    .get("prompt_eval_cached_count")
                    .and_then(Value::as_u64);
                self.generation_rate = rate(
                    self.output_tokens,
                    number(value, "eval_duration").map(|n| n / 1e9),
                );
                // Duration describes uncached prefill. Never divide total prompt
                // tokens by this duration when the cache count is unavailable.
                self.prompt_rate = rate(
                    self.prompt_tokens
                        .zip(self.cached_prompt)
                        .and_then(|(all, cached)| all.checked_sub(cached)),
                    number(value, "prompt_eval_duration").map(|n| n / 1e9),
                );
                self.load_ms = number(value, "load_duration").map(|n| n / 1e6);
            }
        } else {
            if let Some(usage) = value.get("usage") {
                self.output_tokens = usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .or(self.output_tokens);
                self.prompt_tokens = usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .or(self.prompt_tokens);
                self.cached_prompt = usage
                    .pointer("/prompt_tokens_details/cached_tokens")
                    .and_then(Value::as_u64)
                    .or(self.cached_prompt);
            }
            if let Some(timing) = value.get("timings") {
                let predicted = timing.get("predicted_n").and_then(Value::as_u64);
                if let Some(r) = rate(
                    predicted,
                    number(timing, "predicted_ms").map(|n| n / 1000.0),
                ) {
                    self.generation_rate = Some(r);
                    self.output_tokens = predicted.or(self.output_tokens);
                    self.native_timing = true;
                }
                self.prompt_rate = rate(
                    timing.get("prompt_n").and_then(Value::as_u64),
                    number(timing, "prompt_ms").map(|n| n / 1000.0),
                )
                .or(self.prompt_rate);
            }
            self.finish_reason |=
                value
                    .get("choices")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| {
                        rows.iter()
                            .any(|row| row.get("finish_reason").and_then(Value::as_str).is_some())
                    });
        }
        Ok(())
    }
}

struct Decoder {
    pending: Vec<u8>,
    event_data: String,
    total: usize,
    ollama: bool,
    maximum: usize,
}
impl Decoder {
    fn new(ollama: bool) -> Self {
        Self {
            pending: Vec::new(),
            event_data: String::new(),
            total: 0,
            ollama,
            maximum: MAX_STREAM,
        }
    }
    fn feed(
        &mut self,
        data: &[u8],
        stats: &mut StreamStats,
        start: Instant,
    ) -> std::result::Result<(), Issue> {
        self.total = self.total.saturating_add(data.len());
        if self.total > self.maximum {
            return Err(Issue::TooLarge);
        }
        for &byte in data {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.pending);
                let line = std::str::from_utf8(&line)
                    .map_err(|_| Issue::Malformed)?
                    .trim_end_matches('\r');
                self.line(line, stats, start)?;
            } else {
                if self.pending.len() >= MAX_LINE {
                    return Err(Issue::TooLarge);
                }
                self.pending.push(byte);
            }
        }
        Ok(())
    }
    fn line(
        &mut self,
        line: &str,
        stats: &mut StreamStats,
        start: Instant,
    ) -> std::result::Result<(), Issue> {
        if self.ollama {
            if !line.trim().is_empty() {
                stats.event(
                    &serde_json::from_str::<Value>(line).map_err(|_| Issue::Malformed)?,
                    true,
                    start,
                )?;
            }
        } else if line.is_empty() {
            self.flush(stats, start)?;
        } else if let Some(data) = line.strip_prefix("data:") {
            if !self.event_data.is_empty() {
                self.event_data.push('\n');
            }
            self.event_data
                .push_str(data.strip_prefix(' ').unwrap_or(data));
            if self.event_data.len() > MAX_LINE {
                return Err(Issue::TooLarge);
            }
        } else if !line.starts_with(':')
            && !line.starts_with("event:")
            && !line.starts_with("id:")
            && !line.starts_with("retry:")
        {
            return Err(Issue::Malformed);
        }
        Ok(())
    }
    fn flush(&mut self, stats: &mut StreamStats, start: Instant) -> std::result::Result<(), Issue> {
        let data = std::mem::take(&mut self.event_data);
        if data == "[DONE]" {
            stats.finished = true;
        } else if !data.is_empty() {
            stats.event(
                &serde_json::from_str::<Value>(&data).map_err(|_| Issue::Malformed)?,
                false,
                start,
            )?;
        }
        Ok(())
    }
    fn finish(
        &mut self,
        stats: &mut StreamStats,
        start: Instant,
    ) -> std::result::Result<(), Issue> {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            let line = std::str::from_utf8(&line)
                .map_err(|_| Issue::Malformed)?
                .trim_end_matches('\r');
            self.line(line, stats, start)?;
        }
        self.flush(stats, start)?;
        if !stats.finished && !(stats.finish_reason && !self.ollama) {
            return Err(Issue::Malformed);
        }
        Ok(())
    }
}

/// Ephemeral parser shared with the explicitly enabled observation proxy.
/// Streaming retains one bounded framing buffer; nonstreaming retains a bounded
/// JSON response until validation. Neither is persisted or passed to callbacks.
pub(crate) struct ObservationParser {
    decoder: Decoder,
    stats: StreamStats,
    json_body: Vec<u8>,
    streaming: bool,
    start: Instant,
}

impl ObservationParser {
    pub(crate) fn new(ollama: bool, streaming: bool, start: Instant) -> Self {
        let mut decoder = Decoder::new(ollama);
        decoder.maximum = 16 * 1024 * 1024;
        Self {
            decoder,
            stats: StreamStats::default(),
            json_body: Vec::new(),
            streaming,
            start,
        }
    }
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        if self.streaming {
            self.decoder.feed(bytes, &mut self.stats, self.start)?;
        } else {
            if self.json_body.len().saturating_add(bytes.len()) > MAX_BODY {
                bail!("Risposta JSON oltre il limite");
            }
            self.json_body.extend_from_slice(bytes);
        }
        Ok(())
    }
    pub(crate) fn finish(mut self, runtime: &str, model: &str) -> Result<BenchmarkResult> {
        if self.streaming {
            self.decoder.finish(&mut self.stats, self.start)?;
        } else {
            let value: Value = serde_json::from_slice(&self.json_body)
                .map_err(|_| anyhow!("Risposta JSON non valida"))?;
            self.stats.event(&value, self.decoder.ollama, self.start)?;
            if !self.stats.finished && !self.stats.finish_reason {
                bail!("Risposta inferenza incompleta");
            }
            // A buffered JSON response does not expose arrival of the first token.
            self.stats.first_token_ms = None;
        }
        let wall_seconds = self.start.elapsed().as_secs_f64();
        let mut note = "Richiesta esterna osservata tramite proxy locale esplicito; non è un benchmark sintetico. ".to_string();
        if self.stats.native_timing {
            note.push_str("Decode da contatori/tempi nativi. ");
        } else {
            self.stats.generation_rate = rate(self.stats.output_tokens, Some(wall_seconds));
            note.push_str("Tok/s end-to-end = usage output / durata proxy, include prefill, coda e caricamento. ");
        }
        if self.stats.output_tokens.is_none() {
            note.push_str("Token non forniti: conteggio e velocità n/d. ");
        }
        if self.decoder.ollama && self.stats.cached_prompt.is_none() {
            note.push_str("Prefill n/d: contatore cache assente. ");
        }
        if let Some(cached) = self.stats.cached_prompt {
            note.push_str(&format!("Token in cache: {cached}. "));
        }
        if !self.streaming {
            note.push_str("TTFT n/d per risposta non streaming. ");
        }
        note.push_str("TTFT e durata includono lettura richiesta e verifiche proxy. Prompt e testo generato non conservati; richieste che non attraversano questo proxy non sono osservate.");
        Ok(BenchmarkResult {
            runtime: canonical_runtime(runtime)?.into(),
            model: safe_text(model),
            throughput_kind: if self.stats.generation_rate.is_none() {
                "unavailable"
            } else if self.stats.native_timing {
                "native_decode"
            } else {
                "end_to_end"
            }
            .into(),
            output_tokens: self.stats.output_tokens,
            prompt_tokens: self.stats.prompt_tokens,
            wall_seconds,
            generation_tokens_per_sec: self.stats.generation_rate,
            prompt_tokens_per_sec: self.stats.prompt_rate,
            time_to_first_token_ms: self.stats.first_token_ms,
            load_ms: self.stats.load_ms,
            note,
        })
    }
}

/// Run a single fixed Italian synthetic prompt, with at most 64 output tokens.
/// The caller must explicitly request inference. No arbitrary user prompt or
/// generated text is retained. Cancellation drops the in-flight connection.
pub fn benchmark(
    runtime: &str,
    endpoint: &str,
    model: &str,
    max_tokens: u32,
    cancel: Arc<AtomicBool>,
) -> Result<BenchmarkResult> {
    let base = local_url(endpoint)?;
    if model.is_empty() || model.len() > 512 || model.chars().any(char::is_control) {
        bail!("Identificativo del modello non valido");
    }
    let runtime_name = match runtime
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
        .as_str()
    {
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        "llama.cpp" | "llamacpp" => "llama.cpp",
        _ => bail!("Runtime non supportato: scegliere Ollama, LM Studio o llama.cpp"),
    };
    if cancel.load(Ordering::Relaxed) {
        bail!("Benchmark annullato");
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    let executor = executor()?;
    let client = client_for(false, runtime_name)?;
    executor.block_on(async {
        let ollama = runtime_name == "Ollama";
        if ollama {
            ensure_local_ollama_model(&client, &base, model, &cancel, deadline).await?;
        }
        let tokens = max_tokens.clamp(1, 64);
        let body = if ollama {
            json!({"model":model,"prompt":SYNTHETIC_PROMPT,"stream":true,"options":{"num_predict":tokens,"temperature":0,"seed":42}})
        } else {
            json!({"model":model,"messages":[{"role":"user","content":SYNTHETIC_PROMPT}],"max_tokens":tokens,"temperature":0,"stream":true,"stream_options":{"include_usage":true}})
        };
        let start = Instant::now();
        let path = if ollama { "/api/generate" } else { "/v1/chat/completions" };
        let mut response = cancellable(client.post(route(&base, path)).json(&body).send(), &cancel, deadline).await?;
        if !response.status().is_success() { return Err(Issue::Http(response.status().as_u16()).into()); }
        if response.content_length().is_some_and(|n| n > MAX_STREAM as u64) { return Err(Issue::TooLarge.into()); }
        let mut stats = StreamStats::default();
        let mut decoder = Decoder::new(ollama);
        while let Some(chunk) = cancellable(response.chunk(), &cancel, deadline).await? {
            decoder.feed(&chunk, &mut stats, start)?;
            if stats.finished { break; }
        }
        decoder.finish(&mut stats, start)?;
        let wall_seconds = start.elapsed().as_secs_f64();
        let mut note = if stats.native_timing {
            "Decode: tempi nativi del server; TTFT: primo contenuto/thinking ricevuto dal client, include coda e caricamento. ".to_string()
        } else {
            stats.generation_rate = rate(stats.output_tokens, Some(wall_seconds));
            "Tok/s end-to-end = token usage / durata client (include prefill, coda e caricamento); non è la velocità pura di decode. ".to_string()
        };
        if stats.output_tokens.is_none() { note.push_str("Usage token assente: conteggio e tok/s non disponibili. "); }
        if ollama && stats.cached_prompt.is_none() { note.push_str("Prefill n/d: contatore cache non dichiarato dal runtime. "); }
        if let Some(cached) = stats.cached_prompt { note.push_str(&format!("Token prompt in cache: {cached}; cache e stato caldo/freddo influenzano il confronto. ")); }
        note.push_str("Prompt sintetico fisso, seed Ollama 42, massimo 64 token; nessuna risposta conservata. Verificare l'instradamento del runtime locale.");
        Ok(BenchmarkResult {
            runtime: runtime_name.into(), model: safe_text(model), output_tokens: stats.output_tokens,
            throughput_kind: if stats.generation_rate.is_none() { "unavailable" } else if stats.native_timing { "native_decode" } else { "end_to_end" }.into(),
            prompt_tokens: stats.prompt_tokens, wall_seconds,
            generation_tokens_per_sec: stats.generation_rate, prompt_tokens_per_sec: stats.prompt_rate,
            time_to_first_token_ms: stats.first_token_ms, load_ms: stats.load_ms, note,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    /// Real loopback HTTP exchange, including the request body, without a runtime install.
    fn mock(
        responses: Vec<(u16, &'static str, String)>,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let task = thread::spawn(move || {
            for (status, content_type, body) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = socket.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&request).to_string());
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (endpoint, rx, task)
    }

    fn json_response(body: &str) -> (u16, &'static str, String) {
        (200, "application/json", body.into())
    }
    fn cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn endpoints_are_strictly_loopback_origins() {
        for endpoint in [
            "http://127.0.0.1:1234",
            "http://[::1]:1234",
            "http://localhost:11434/",
            "http://127.1.2.3:8080",
        ] {
            assert!(validate_endpoint(endpoint).is_ok(), "{endpoint}");
        }
        for endpoint in [
            "https://127.0.0.1",
            "http://example.com",
            "http://192.168.1.2:8080",
            "http://0.0.0.0",
            "http://[::ffff:127.0.0.1]",
            "http://localhost.evil",
            "http://localhost.:1234",
            "http://user:pass@localhost",
            "http://@localhost",
            "http://localhost/v1",
            "http://localhost/a/../",
            "http://localhost/..",
            "http://localhost/?x=1",
            "http://localhost/#x",
            " http://localhost",
            "http://localhost:0",
        ] {
            assert!(validate_endpoint(endpoint).is_err(), "{endpoint}");
        }
        assert_eq!(
            local_url("http://localhost:1234").unwrap().host_str(),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn ollama_probe_merges_installed_and_loaded_without_inference() {
        let (endpoint, requests, task) = mock(vec![
            json_response(
                r#"{"models":[{"name":"local:latest","size":123,"details":{"quantization_level":"Q4_K_M"}},{"name":"remote","size":999,"remote_host":"https://remote.example"}]}"#,
            ),
            json_response(
                r#"{"models":[{"name":"local:latest","size_vram":100,"context_length":4096}]}"#,
            ),
        ]);
        let result = probe("Ollama", &endpoint);
        task.join().unwrap();
        assert_eq!(result.status, "online");
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].loaded, Some(true));
        assert_eq!(result.models[0].vram_bytes, Some(100));
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.starts_with("GET /api/"))
        );
    }

    #[test]
    fn studio_native_reports_loaded_instance_and_skips_embeddings() {
        let (endpoint, _, task) = mock(vec![json_response(
            r#"{"models":[{"type":"llm","key":"file-id","size_bytes":123,"loaded_instances":[{"id":"inference-id","config":{"context_length":8192}}],"quantization":{"name":"Q4"}},{"type":"embedding","key":"embed"}]}"#,
        )]);
        let result = probe("LM Studio", &endpoint);
        task.join().unwrap();
        assert_eq!(result.status, "online");
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].id, "inference-id");
        assert_eq!(result.models[0].loaded, Some(true));
        assert_eq!(result.models[0].context_length, Some(8192));
    }

    #[test]
    fn studio_falls_back_only_for_unsupported_native_route() {
        let (endpoint, requests, task) = mock(vec![
            (404, "application/json", "{}".into()),
            json_response(r#"{"data":[{"id":"local"}]}"#),
        ]);
        let result = probe("LM Studio", &endpoint);
        task.join().unwrap();
        assert_eq!(result.status, "online");
        assert_eq!(result.models[0].loaded, None);
        assert!(
            requests
                .try_iter()
                .last()
                .unwrap()
                .starts_with("GET /v1/models ")
        );
    }

    #[test]
    fn auth_error_and_malformed_are_not_reported_offline() {
        for (code, body, expected) in [
            (401, "{}", "auth"),
            (500, "{}", "error"),
            (200, "<html>no</html>", "error"),
            (200, "{}", "error"),
            (503, "{}", "loading"),
        ] {
            let (endpoint, _, task) = mock(vec![(code, "application/json", body.into())]);
            let result = probe("LM Studio", &endpoint);
            task.join().unwrap();
            assert_eq!(result.status, expected);
        }
    }

    #[test]
    fn refusal_is_offline_and_remote_is_invalid() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(probe("Ollama", &endpoint).status, "offline");
        assert_eq!(probe("Ollama", "http://203.0.113.1:8080").status, "invalid");
        assert!(benchmark("Ollama", "http://example.com", "x", 64, cancel()).is_err());
    }

    #[test]
    fn ollama_stream_uses_native_durations_and_uncached_prefill() {
        let stream = concat!(
            "{\"response\":\"Ciao\",\"done\":false}\n",
            "{\"done\":true,\"eval_count\":10,\"eval_duration\":500000000,\"prompt_eval_count\":20,\"prompt_eval_cached_count\":15,\"prompt_eval_duration\":250000000,\"load_duration\":1000000}\n"
        );
        let (endpoint, requests, task) = mock(vec![
            json_response(r#"{"models":[{"name":"local","size":100}]}"#),
            (200, "application/x-ndjson", stream.into()),
        ]);
        let result = benchmark("Ollama", &endpoint, "local", 1000, cancel()).unwrap();
        task.join().unwrap();
        assert_eq!(result.output_tokens, Some(10));
        assert_eq!(result.generation_tokens_per_sec, Some(20.0));
        assert_eq!(result.prompt_tokens_per_sec, Some(20.0));
        assert_eq!(result.load_ms, Some(1.0));
        assert!(result.time_to_first_token_ms.is_some());
        let body = requests.try_iter().last().unwrap();
        let body: Value = serde_json::from_str(body.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["options"]["num_predict"], 64);
        assert_eq!(body["prompt"], SYNTHETIC_PROMPT);
    }

    #[test]
    fn sse_uses_real_usage_and_never_counts_characters() {
        for with_usage in [true, false] {
            let mut stream = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Una frase di molte lettere\"}}]}\n\n".to_string();
            if with_usage {
                stream.push_str("data: {\"choices\":[],\"usage\":{\"completion_tokens\":7,\"prompt_tokens\":12}}\n\n");
            }
            stream.push_str("data: [DONE]\n\n");
            let (endpoint, _, task) = mock(vec![(200, "text/event-stream", stream)]);
            let result = benchmark("LM Studio", &endpoint, "local", 64, cancel()).unwrap();
            task.join().unwrap();
            assert_eq!(
                result.output_tokens,
                if with_usage { Some(7) } else { None }
            );
            assert_eq!(result.generation_tokens_per_sec.is_some(), with_usage);
            assert!(result.time_to_first_token_ms.is_some());
        }
    }

    #[test]
    fn remote_ollama_preflight_refuses_before_generate() {
        let (endpoint, requests, task) = mock(vec![json_response(
            r#"{"models":[{"name":"alias","size":1,"remote_host":"https://example.com"}]}"#,
        )]);
        assert!(benchmark("Ollama", &endpoint, "alias", 64, cancel()).is_err());
        task.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
    }

    #[test]
    fn truncated_or_invalid_stream_is_an_error() {
        for stream in [
            "data: {bad}\n\n",
            "data: {\"choices\":[]}\n\n",
            "{\"choices\":[]}",
            "data: [DONE]\n\n",
        ] {
            let (endpoint, _, task) = mock(vec![(200, "text/event-stream", stream.into())]);
            let result = benchmark("llama.cpp", &endpoint, "local", 64, cancel());
            task.join().unwrap();
            if stream.contains("[DONE]") {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn stream_decoder_handles_utf8_split_and_crlf() {
        let start = Instant::now();
        let mut stats = StreamStats::default();
        let mut decoder = Decoder::new(false);
        let stream =
            "data: {\"choices\":[{\"delta\":{\"content\":\"caffè\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n";
        for byte in stream.as_bytes() {
            decoder.feed(&[*byte], &mut stats, start).unwrap();
        }
        decoder.finish(&mut stats, start).unwrap();
        assert!(stats.first_token_ms.is_some());
    }

    #[test]
    fn cancellation_interrupts_a_stalled_http_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (accepted, ready) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let task = thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            accepted.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(4)).unwrap();
        });
        let flag = cancel();
        let worker_flag = flag.clone();
        let worker =
            thread::spawn(move || benchmark("LM Studio", &endpoint, "local", 64, worker_flag));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        flag.store(true, Ordering::Relaxed);
        assert!(
            worker
                .join()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("annullato")
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        release.send(()).unwrap();
        task.join().unwrap();
    }

    #[test]
    fn limits_and_terminal_controls_are_bounded() {
        let (endpoint, _, task) = mock(vec![(200, "application/json", "x".repeat(MAX_BODY + 1))]);
        assert_eq!(probe("LM Studio", &endpoint).status, "error");
        task.join().unwrap();
        let mut decoder = Decoder::new(false);
        assert!(
            decoder
                .feed(
                    &vec![b'x'; MAX_LINE + 1],
                    &mut StreamStats::default(),
                    Instant::now()
                )
                .is_err()
        );
        assert_eq!(safe_text("name\u{1b}\n\u{202e}rest"), "namerest");
    }

    #[test]
    fn http_redirects_are_returned_as_errors_without_following_location() {
        let trap = TcpListener::bind("127.0.0.1:0").unwrap();
        trap.set_nonblocking(true).unwrap();
        let location = format!("http://{}/api/v1/models", trap.local_addr().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            write!(socket, "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let result = probe("LM Studio", &endpoint);
        task.join().unwrap();
        assert_eq!(result.status, "error");
        assert!(result.message.contains("302"));
        assert!(
            matches!(trap.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    #[test]
    fn malformed_ollama_ps_preserves_inventory_with_unknown_loaded_state() {
        let (endpoint, requests, task) = mock(vec![
            json_response(r#"{"models":[{"name":"local","size":123}]}"#),
            json_response(r#"{"models":"not an array"}"#),
        ]);
        let result = probe("Ollama", &endpoint);
        task.join().unwrap();
        assert_eq!(result.status, "online");
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].loaded, None);
        assert_eq!(result.models[0].vram_bytes, None);
        assert!(result.message.contains("caricati n/d"));
        assert_eq!(requests.try_iter().count(), 2);
    }

    #[test]
    fn ollama_missing_cache_counter_does_not_fabricate_prefill_rate() {
        let stream = "{\"response\":\"Ciao\",\"done\":true,\"eval_count\":10,\"eval_duration\":500000000,\"prompt_eval_count\":100,\"prompt_eval_duration\":1000000}\n";
        let (endpoint, _, task) = mock(vec![
            json_response(r#"{"models":[{"name":"local","size":100}]}"#),
            (200, "application/x-ndjson", stream.into()),
        ]);
        let result = benchmark("Ollama", &endpoint, "local", 64, cancel()).unwrap();
        task.join().unwrap();
        assert_eq!(result.generation_tokens_per_sec, Some(20.0));
        assert_eq!(result.prompt_tokens, Some(100));
        assert_eq!(result.prompt_tokens_per_sec, None);
        assert!(result.note.contains("cache non dichiarato"));
    }

    #[test]
    fn cancellation_during_a_chunked_sse_stream_drops_pending_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (sent, received) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|text| text.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let chunk = "data: {\"choices\":[{\"delta\":{\"content\":\"Ciao\"}}]}\n\n";
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{chunk}\r\n", chunk.len()).unwrap();
            socket.flush().unwrap();
            sent.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(4)).unwrap();
        });
        let flag = cancel();
        let worker_flag = flag.clone();
        let worker =
            thread::spawn(move || benchmark("LM Studio", &endpoint, "local", 64, worker_flag));
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        flag.store(true, Ordering::Relaxed);
        let result = worker.join().unwrap();
        release.send(()).unwrap();
        server.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("annullato"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn ollama_discovery_excludes_known_encoders_without_broad_name_matching() {
        let (endpoint, requests, task) = mock(vec![
            json_response(
                r#"{"models":[
                {"name":"nomic-embed-text:latest","size":100},
                {"name":"local/nomic-embed-text:custom","size":100},
                {"name":"renamed-encoder:latest","size":100,"details":{"family":"BERT"}},
                {"name":"other-encoder:latest","size":100,"details":{"families":["nomic-bert"]}},
                {"name":"nomic-embed-text-explainer:latest","size":100,"details":{"family":"llama"}},
                {"name":"chat:latest","size":100,"details":{"families":["qwen2"]}}
            ]}"#,
            ),
            json_response(r#"{"models":[]}"#),
        ]);
        let result = probe("Ollama", &endpoint);
        task.join().unwrap();
        assert_eq!(result.status, "online");
        assert_eq!(
            result
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["nomic-embed-text-explainer:latest", "chat:latest"]
        );
        assert!(result.message.contains("4 embedding esclusi"));
        assert_eq!(requests.try_iter().count(), 2);
    }

    #[test]
    fn ollama_encoder_benchmark_preflight_never_posts_generation() {
        for model in [
            json!({"name":"nomic-embed-text:latest","size":100}),
            json!({"name":"renamed-encoder","size":100,"details":{"family":"bert"}}),
            json!({"name":"renamed-encoder","size":100,"details":{"families":["nomic-bert"]}}),
        ] {
            let body = json!({"models":[model.clone()]}).to_string();
            let (endpoint, requests, task) = mock(vec![(200, "application/json", body)]);
            let error = benchmark(
                "Ollama",
                &endpoint,
                model["name"].as_str().unwrap(),
                64,
                cancel(),
            )
            .unwrap_err();
            task.join().unwrap();
            assert!(error.to_string().contains("embedding non generativo"));
            let requests = requests.try_iter().collect::<Vec<_>>();
            assert_eq!(requests.len(), 1);
            assert!(requests[0].starts_with("GET /api/tags "));
        }
    }

    #[test]
    fn runtime_bearer_auth_is_sensitive_and_reaches_only_local_http() {
        let secret = "synthetic-test-token-not-a-real-key";
        let headers = authorization(Some(secret.into())).unwrap();
        assert!(headers[reqwest::header::AUTHORIZATION].is_sensitive());
        let (endpoint, requests, task) = mock(vec![json_response(r#"{"data":[]}"#)]);
        let client = build_client(true, headers).unwrap();
        let executor = executor().unwrap();
        let base = local_url(&endpoint).unwrap();
        let value = executor
            .block_on(get_json(
                &client,
                &base,
                "/v1/models",
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(2),
            ))
            .unwrap();
        assert!(value["data"].is_array());
        task.join().unwrap();
        assert!(
            requests
                .recv()
                .unwrap()
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {secret}"))
        );
        let invalid = authorization(Some("private\r\nInjected: no".into())).unwrap_err();
        assert!(!invalid.to_string().contains("private"));
    }
}
