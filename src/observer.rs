//! Explicit loopback recording proxy for local LLM traffic.
//!
//! Only requests routed through this proxy are observed. Prompt/output bytes are
//! forwarded and parsed ephemerally, never logged or passed to the callback.
//! This is not a general reverse proxy: admin, downloads, tools, arbitrary paths
//! and upstream URLs are deliberately unsupported. Local users can access this
//! listener; its upstream credential is read only from the runtime environment.

use crate::{llm, model::BenchmarkResult};
use anyhow::{Result, anyhow, bail};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::Response,
};
use futures_util::{
    future::{Either, select},
    stream,
};
use std::{
    future::{Future, IntoFuture},
    net::{Ipv4Addr, TcpListener},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_REQUEST: usize = 1024 * 1024;
const MAX_METADATA: usize = 2 * 1024 * 1024;
const MAX_RESPONSE: usize = 16 * 1024 * 1024;
type Callback = Arc<dyn Fn(BenchmarkResult) + Send + Sync>;

const MAX_TOKEN: usize = 8192;
// Deliberately no Debug/Serialize implementations for credentials.
struct ObserverToken {
    bytes: [u8; MAX_TOKEN],
    length: u64,
}

fn front_auth_policy(
    upstream_protected: bool,
    token: Option<String>,
) -> Result<Option<ObserverToken>> {
    let token = token.filter(|token| !token.is_empty());
    if upstream_protected && token.is_none() {
        bail!("Runtime protetto: configurare RUST_CPU_OBSERVER_TOKEN prima di avviare l'observer");
    }
    token.map(|token| {
        if token.len() > MAX_TOKEN || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
            bail!("RUST_CPU_OBSERVER_TOKEN non valido: usare un token ASCII senza spazi o controlli, massimo 8192 byte");
        }
        let mut bytes = [0; MAX_TOKEN];
        bytes[..token.len()].copy_from_slice(token.as_bytes());
        Ok(ObserverToken { bytes, length: token.len() as u64 })
    }).transpose()
}

fn constant_time_matches(expected: &ObserverToken, supplied: &[u8]) -> bool {
    if supplied.len() > MAX_TOKEN {
        return false;
    }
    let mut bytes = [0; MAX_TOKEN];
    bytes[..supplied.len()].copy_from_slice(supplied);
    bool::from(expected.bytes.ct_eq(&bytes) & expected.length.ct_eq(&(supplied.len() as u64)))
}

struct ProxyAuth {
    client: reqwest::Client,
    frontend: Option<ObserverToken>,
}
impl ProxyAuth {
    fn new(upstream_token: Option<String>, front_token: Option<String>) -> Result<Self> {
        let protected = upstream_token
            .as_ref()
            .is_some_and(|token| !token.is_empty());
        let frontend = front_auth_policy(protected, front_token)?;
        Ok(Self {
            client: llm::client_with_token(false, upstream_token)?,
            frontend,
        })
    }
    fn from_environment(runtime: &str) -> Result<Self> {
        Self::new(
            llm::runtime_token(runtime)?,
            llm::environment_token("RUST_CPU_OBSERVER_TOKEN")?,
        )
    }
}

struct Proxy {
    runtime: &'static str,
    upstream: reqwest::Url,
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    stop: Arc<AtomicBool>,
    callback: Callback,
    port: u16,
    frontend: Option<ObserverToken>,
}

/// Serve until `stop` is set. Bind only 127.0.0.1, with four concurrent jobs.
/// Metrics contain model/timing/counts only. An aborted or invalid response does
/// not produce a successful observation. Upstream auth uses per-runtime env vars;
/// inbound Authorization/Cookie/Host headers are never forwarded upstream.
/// `on_metric` must return promptly (normally enqueue to a persistence worker);
/// a blocking callback can delay downstream EOF and graceful shutdown.
pub fn serve(
    runtime: &str,
    endpoint: &str,
    port: u16,
    on_metric: impl Fn(BenchmarkResult) + Send + Sync + 'static,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    if port == 0 {
        bail!("Specificare una porta observer da 1 a 65535");
    }
    let upstream = llm::local_url(endpoint)?;
    let runtime = llm::canonical_runtime(runtime)?;
    if upstream.host_str() == Some("127.0.0.1") && upstream.port_or_known_default() == Some(port) {
        bail!("La porta observer deve essere diversa da quella del runtime");
    }
    let auth = ProxyAuth::from_environment(runtime)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|_| anyhow!("Impossibile aprire la porta observer su 127.0.0.1"))?;
    serve_bound(runtime, upstream, listener, Arc::new(on_metric), stop, auth)
}

fn serve_bound(
    runtime: &'static str,
    upstream: reqwest::Url,
    listener: TcpListener,
    callback: Callback,
    stop: Arc<AtomicBool>,
    auth: ProxyAuth,
) -> Result<()> {
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let executor = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| anyhow!("Impossibile avviare il runtime observer"))?;
    executor.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let state = Arc::new(Proxy {
            runtime,
            upstream,
            client: auth.client,
            slots: Arc::new(Semaphore::new(4)),
            stop: stop.clone(),
            callback,
            port,
            frontend: auth.frontend,
        });
        let app = Router::new().fallback(forward).with_state(state);
        let graceful_stop = stop.clone();
        let server = axum::serve(listener, app).with_graceful_shutdown(wait_stop(graceful_stop));
        let bounded_stop = async move {
            wait_stop(stop).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        match select(Box::pin(server.into_future()), Box::pin(bounded_stop)).await {
            Either::Left((result, _)) => result.map_err(|_| anyhow!("Errore del server observer")),
            // Runtime drop aborts any remaining slow downstream connection.
            Either::Right(_) => Ok(()),
        }
    })
}

async fn wait_stop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn reply(status: StatusCode, message: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(message))
        .expect("constant response")
}

fn allowed(runtime: &str, method: &Method, path: &str) -> bool {
    match (runtime, method.as_str()) {
        ("Ollama", "GET") => matches!(path, "/api/tags" | "/api/ps" | "/api/version"),
        ("Ollama", "POST") => matches!(path, "/api/generate" | "/api/chat"),
        ("LM Studio", "GET") => matches!(path, "/api/v1/models" | "/v1/models"),
        ("LM Studio", "POST") | ("llama.cpp", "POST") => path == "/v1/chat/completions",
        ("llama.cpp", "GET") => matches!(path, "/health" | "/v1/health" | "/v1/models"),
        _ => false,
    }
}

async fn wait_request<T>(
    future: impl Future<Output = T>,
    stop: &AtomicBool,
    deadline: Instant,
) -> Result<T> {
    let mut future = std::pin::pin!(future);
    loop {
        if stop.load(Ordering::Relaxed) {
            bail!("Observer arrestato");
        }
        if Instant::now() >= deadline {
            bail!("Timeout richiesta observer");
        }
        if let Ok(result) = tokio::time::timeout(Duration::from_millis(50), &mut future).await {
            return Ok(result);
        }
    }
}

async fn forward(State(proxy): State<Arc<Proxy>>, request: Request) -> Response {
    if proxy.stop.load(Ordering::Relaxed) {
        return reply(StatusCode::SERVICE_UNAVAILABLE, "Observer in arresto");
    }
    if let Some(expected) = &proxy.frontend {
        let valid = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|token| constant_time_matches(expected, token.as_bytes()));
        if !valid {
            let mut response = reply(
                StatusCode::UNAUTHORIZED,
                "Autenticazione observer richiesta",
            );
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                "Bearer realm=\"RUST-CPU observer\""
                    .parse()
                    .expect("constant challenge"),
            );
            return response;
        }
    }
    let permit = match proxy.slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return reply(
                StatusCode::TOO_MANY_REQUESTS,
                "Observer occupato: massimo quattro richieste concorrenti",
            );
        }
    };
    let (parts, body) = request.into_parts();
    let path = parts.uri.path();
    if parts.uri.query().is_some()
        || parts.uri.scheme().is_some()
        || !allowed(proxy.runtime, &parts.method, path)
    {
        return reply(StatusCode::NOT_FOUND, "Rotta observer non consentita");
    }
    let host_valid = parts
        .headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|host| llm::local_url(&format!("http://{host}")).ok())
        .is_some_and(|host| host.port_or_known_default() == Some(proxy.port));
    if !host_valid {
        return reply(StatusCode::BAD_REQUEST, "Host observer non valido");
    }
    // No CORS: a remote browser page cannot obtain privileged loopback access.
    if parts.headers.contains_key(header::ORIGIN) {
        return reply(
            StatusCode::FORBIDDEN,
            "Richieste browser cross-origin non supportate",
        );
    }
    let declared_length = match parts.headers.get(header::CONTENT_LENGTH) {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|text| text.parse::<usize>().ok())
        {
            Some(length) => length,
            None => return reply(StatusCode::BAD_REQUEST, "Content-Length non valido"),
        },
        None => 0,
    };
    if declared_length > MAX_REQUEST {
        return reply(StatusCode::PAYLOAD_TOO_LARGE, "Richiesta oltre 1 MiB");
    }
    let start = Instant::now();
    let deadline = start + Duration::from_secs(120);
    let inference = parts.method == Method::POST;
    let mut model = String::new();
    let mut streaming = false;
    let request_body = if inference {
        let content_type = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json")
        {
            return reply(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Serve Content-Type application/json",
            );
        }
        let bytes = match wait_request(
            to_bytes(body, MAX_REQUEST),
            &proxy.stop,
            start + Duration::from_secs(30),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => {
                return reply(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Corpo richiesta non valido o oltre 1 MiB",
                );
            }
            Err(_) => return reply(StatusCode::REQUEST_TIMEOUT, "Timeout lettura richiesta"),
        };
        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => return reply(StatusCode::BAD_REQUEST, "Richiesta JSON non valida"),
        };
        model = match value
            .get("model")
            .and_then(serde_json::Value::as_str)
            .filter(|model| {
                !model.is_empty() && model.len() <= 512 && !model.chars().any(char::is_control)
            }) {
            Some(model) => model.to_string(),
            None => return reply(StatusCode::BAD_REQUEST, "Identificativo modello richiesto"),
        };
        streaming = match value.get("stream") {
            Some(value) => match value.as_bool() {
                Some(value) => value,
                None => return reply(StatusCode::BAD_REQUEST, "stream deve essere booleano"),
            },
            None => proxy.runtime == "Ollama",
        };
        if proxy.runtime == "Ollama"
            && llm::ensure_local_ollama_model(
                &proxy.client,
                &proxy.upstream,
                &model,
                &proxy.stop,
                deadline,
            )
            .await
            .is_err()
        {
            return reply(
                StatusCode::FORBIDDEN,
                "Modello Ollama remoto, embedding, non installato o non verificabile",
            );
        }
        bytes
    } else {
        if declared_length != 0 || parts.headers.contains_key(header::TRANSFER_ENCODING) {
            return reply(
                StatusCode::BAD_REQUEST,
                "GET metadata senza corpo richiesto",
            );
        }
        Bytes::new()
    };
    let mut upstream_url = proxy.upstream.clone();
    upstream_url.set_path(path);
    let mut builder = proxy.client.request(parts.method, upstream_url);
    if inference {
        builder = builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(request_body);
    }
    let upstream = match llm::cancellable(builder.send(), &proxy.stop, deadline).await {
        Ok(response) => response,
        Err(_) => {
            return reply(
                StatusCode::BAD_GATEWAY,
                "Runtime locale non disponibile o timeout",
            );
        }
    };
    let status = upstream.status();
    if status.is_redirection() {
        return reply(
            StatusCode::BAD_GATEWAY,
            "Redirect del runtime locale rifiutato",
        );
    }
    let limit = if inference {
        MAX_RESPONSE
    } else {
        MAX_METADATA
    };
    if upstream
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return reply(StatusCode::BAD_GATEWAY, "Risposta runtime oltre il limite");
    }
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let content_encoding = upstream.headers().get(header::CONTENT_ENCODING).cloned();
    let parser = (inference && status.is_success())
        .then(|| llm::ObservationParser::new(proxy.runtime == "Ollama", streaming, start));
    let state = Forwarding {
        upstream,
        parser,
        proxy,
        model,
        bytes: 0,
        limit,
        deadline,
        _permit: permit,
        done: false,
    };
    let output = stream::unfold(state, |mut state| async move {
        if state.done {
            return None;
        }
        match llm::cancellable(state.upstream.chunk(), &state.proxy.stop, state.deadline).await {
            Ok(Some(bytes)) => {
                state.bytes = state.bytes.saturating_add(bytes.len());
                if state.bytes > state.limit {
                    state.done = true;
                    return Some((
                        Err(std::io::Error::other("Risposta observer oltre il limite")),
                        state,
                    ));
                }
                if state
                    .parser
                    .as_mut()
                    .is_some_and(|parser| parser.feed(&bytes).is_err())
                {
                    state.parser = None;
                }
                Some((Ok(bytes), state))
            }
            Ok(None) => {
                if let Some(parser) = state.parser.take()
                    && let Ok(metric) = parser.finish(state.proxy.runtime, &state.model)
                {
                    // Callback is metadata only and must never receive prompt/output.
                    (state.proxy.callback)(metric);
                }
                None
            }
            Err(_) => {
                state.done = true;
                state.parser = None;
                Some((
                    Err(std::io::Error::other("Trasferimento observer interrotto")),
                    state,
                ))
            }
        }
    });
    let mut response = Response::builder()
        .status(status)
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(content_encoding) = content_encoding {
        response = response.header(header::CONTENT_ENCODING, content_encoding);
    }
    response
        .body(Body::from_stream(output))
        .expect("validated upstream response")
}

struct Forwarding {
    upstream: reqwest::Response,
    parser: Option<llm::ObservationParser>,
    proxy: Arc<Proxy>,
    model: String,
    bytes: usize,
    limit: usize,
    deadline: Instant,
    _permit: OwnedSemaphorePermit,
    done: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpStream,
        sync::mpsc,
        thread,
    };

    struct Harness {
        endpoint: String,
        stop: Arc<AtomicBool>,
        task: Option<thread::JoinHandle<Result<()>>>,
        metrics: mpsc::Receiver<BenchmarkResult>,
    }
    impl Drop for Harness {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(task) = self.task.take() {
                task.join().unwrap().unwrap();
            }
        }
    }
    fn proxy(runtime: &'static str, upstream: &str) -> Harness {
        proxy_with_auth(runtime, upstream, ProxyAuth::new(None, None).unwrap())
    }
    fn proxy_with_auth(runtime: &'static str, upstream: &str, auth: ProxyAuth) -> Harness {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let upstream = llm::local_url(upstream).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let (tx, metrics) = mpsc::channel();
        let callback = Arc::new(move |metric| {
            let _ = tx.send(metric);
        });
        let task =
            thread::spawn(move || serve_bound(runtime, upstream, listener, callback, signal, auth));
        Harness {
            endpoint,
            stop,
            task: Some(task),
            metrics,
        }
    }
    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(4))
            .build()
            .unwrap()
    }
    fn read_request(socket: &mut TcpStream) -> String {
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let count = socket.read(&mut buffer).unwrap();
            assert!(count > 0, "request ended before its declared body");
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .and_then(|text| text.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            assert!(request.len() < MAX_REQUEST + 8192);
        }
        String::from_utf8(request).unwrap()
    }
    fn wire(status: u16, content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }
    fn wire_chunks(chunks: &[&str]) -> String {
        let mut wire = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_string();
        for chunk in chunks {
            wire.push_str(&format!("{:x}\r\n{chunk}\r\n", chunk.len()));
        }
        wire.push_str("0\r\n\r\n");
        wire
    }
    fn upstream(replies: Vec<String>) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let task = thread::spawn(move || {
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(_) => panic!("expected upstream request did not arrive"),
                    }
                };
                let request = read_request(&mut socket);
                let _ = tx.send(request);
                let _ = socket.write_all(reply.as_bytes());
            }
        });
        (endpoint, rx, task)
    }

    #[test]
    fn forwards_chunked_sse_raw_and_records_only_successful_metadata() {
        let first = "data: {\"choices\":[{\"delta\":{\"content\":\"PRIVATE_OUTPUT_é\"}}]}\n\n";
        let last = "data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}],\"usage\":{\"completion_tokens\":5,\"prompt_tokens\":9}}\n\ndata: [DONE]\n\n";
        let (endpoint, requests, server) = upstream(vec![wire_chunks(&[first, last])]);
        let observer = proxy("LM Studio", &endpoint);
        let body = serde_json::json!({"model":"test-local","stream":true,"messages":[{"role":"user","content":"PRIVATE_PROMPT"}]});
        let response = client()
            .post(format!("{}/v1/chat/completions", observer.endpoint))
            .header(header::AUTHORIZATION, "Bearer DOWNSTREAM_SECRET")
            .header(header::COOKIE, "private-cookie=secret")
            .json(&body)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().unwrap(), format!("{first}{last}"));
        let metric = observer
            .metrics
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(metric.model, "test-local");
        assert_eq!(metric.output_tokens, Some(5));
        assert_eq!(metric.throughput_kind, "end_to_end");
        assert!(metric.time_to_first_token_ms.is_some());
        assert!(metric.note.contains("esterna osservata"));
        let serialized = serde_json::to_string(&metric).unwrap();
        for secret in [
            "PRIVATE_PROMPT",
            "PRIVATE_OUTPUT",
            "DOWNSTREAM_SECRET",
            "private-cookie",
        ] {
            assert!(!serialized.contains(secret));
        }
        server.join().unwrap();
        let forwarded = requests.recv().unwrap();
        assert!(forwarded.contains("PRIVATE_PROMPT"));
        assert!(!forwarded.contains("DOWNSTREAM_SECRET"));
        assert!(!forwarded.contains("private-cookie"));
    }

    #[test]
    fn ollama_chat_preflights_local_model_and_uses_native_metrics() {
        let generated = "{\"message\":{\"content\":\"Ciao\"},\"done\":false}\n{\"done\":true,\"eval_count\":4,\"eval_duration\":200000000,\"prompt_eval_count\":10,\"prompt_eval_cached_count\":5,\"prompt_eval_duration\":100000000}\n";
        let (endpoint, requests, server) = upstream(vec![
            wire(
                200,
                "application/json",
                r#"{"models":[{"name":"local:latest","size":123}]}"#,
            ),
            wire(200, "application/x-ndjson", generated),
        ]);
        let observer = proxy("Ollama", &endpoint);
        let response = client().post(format!("{}/api/chat", observer.endpoint))
            .json(&serde_json::json!({"model":"local","stream":true,"messages":[{"role":"user","content":"ciao"}]})).send().unwrap();
        assert_eq!(response.text().unwrap(), generated);
        let metric = observer
            .metrics
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(metric.generation_tokens_per_sec, Some(20.0));
        assert_eq!(metric.throughput_kind, "native_decode");
        assert_eq!(metric.prompt_tokens_per_sec, Some(50.0));
        assert!(metric.time_to_first_token_ms.is_some());
        server.join().unwrap();
        let requests: Vec<_> = requests.try_iter().collect();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/tags "));
        assert!(requests[1].starts_with("POST /api/chat "));
    }

    #[test]
    fn nonstreaming_json_has_usage_but_no_invented_ttft() {
        let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"hello"}}],"usage":{"completion_tokens":3,"prompt_tokens":12}}"#;
        let (endpoint, _, server) = upstream(vec![wire(200, "application/json", body)]);
        let observer = proxy("llama.cpp", &endpoint);
        let response = client()
            .post(format!("{}/v1/chat/completions", observer.endpoint))
            .json(&serde_json::json!({"model":"test","stream":false}))
            .send()
            .unwrap();
        assert_eq!(response.text().unwrap(), body);
        let metric = observer
            .metrics
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(metric.output_tokens, Some(3));
        assert_eq!(metric.time_to_first_token_ms, None);
        server.join().unwrap();
    }

    #[test]
    fn malformed_truncated_or_http_error_never_emits_success_metric() {
        for (status, body) in [
            (200, "data: {bad}\n\n"),
            (
                200,
                "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ),
            (500, "private runtime error"),
            (
                200,
                "data: {\"choices\":[{\"finish_reason\":\"error\"}],\"usage\":{\"completion_tokens\":5}}\n\ndata: [DONE]\n\n",
            ),
        ] {
            let (endpoint, _, server) = upstream(vec![wire(status, "text/event-stream", body)]);
            let observer = proxy("LM Studio", &endpoint);
            let response = client()
                .post(format!("{}/v1/chat/completions", observer.endpoint))
                .json(&serde_json::json!({"model":"test","stream":true}))
                .send()
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.text().unwrap(), body);
            server.join().unwrap();
            assert!(observer.metrics.try_recv().is_err());
        }
    }

    #[test]
    fn rejects_remote_upstream_admin_routes_queries_bad_json_and_large_requests() {
        let stop = Arc::new(AtomicBool::new(true));
        assert!(
            serve(
                "ollama",
                "http://192.0.2.1:11434",
                18434,
                |_| {},
                stop.clone()
            )
            .is_err()
        );
        assert!(serve("ollama", "http://127.0.0.1:18434", 18434, |_| {}, stop).is_err());
        let observer = proxy("Ollama", "http://127.0.0.1:1");
        for path in [
            "/api/pull",
            "/api/delete",
            "/api/create",
            "/api/generate?upstream=http://example.com",
            "/v1/chat/completions",
        ] {
            assert_eq!(
                client()
                    .post(format!("{}{path}", observer.endpoint))
                    .json(&serde_json::json!({"model":"test"}))
                    .send()
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(
            client()
                .post(format!("{}/api/generate", observer.endpoint))
                .header(header::CONTENT_TYPE, "application/json")
                .body("{bad}")
                .send()
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            client()
                .post(format!("{}/api/generate", observer.endpoint))
                .header(header::CONTENT_TYPE, "application/json")
                .body(vec![b'x'; MAX_REQUEST + 1])
                .send()
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            client()
                .get(format!("{}/api/tags", observer.endpoint))
                .header(header::HOST, "example.com")
                .send()
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn known_remote_ollama_model_is_not_forwarded_for_inference() {
        let (endpoint, requests, server) = upstream(vec![wire(
            200,
            "application/json",
            r#"{"models":[{"name":"alias","size":100,"remote_host":"https://example.com"}]}"#,
        )]);
        let observer = proxy("Ollama", &endpoint);
        let response = client()
            .post(format!("{}/api/generate", observer.endpoint))
            .json(&serde_json::json!({"model":"alias","prompt":"PRIVATE"}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        server.join().unwrap();
        assert_eq!(requests.try_iter().count(), 1);
        assert!(observer.metrics.try_recv().is_err());
    }

    #[test]
    fn stop_and_downstream_disconnect_abort_stream_without_success_metric() {
        for disconnect in [true, false] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let (release, released) = mpsc::channel();
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                read_request(&mut socket);
                let first = "data: {\"choices\":[{\"delta\":{\"content\":\"started\"}}]}\n\n";
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{first}\r\n",first.len()).unwrap();
                socket.flush().unwrap();
                released.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            let observer = proxy("LM Studio", &endpoint);
            let mut response = client()
                .post(format!("{}/v1/chat/completions", observer.endpoint))
                .json(&serde_json::json!({"model":"test","stream":true}))
                .send()
                .unwrap();
            let mut first = [0u8; 8];
            assert!(response.read(&mut first).unwrap() > 0);
            if disconnect {
                drop(response);
            } else {
                observer.stop.store(true, Ordering::Relaxed);
                let mut remaining = String::new();
                assert!(response.read_to_string(&mut remaining).is_err());
            }
            observer.stop.store(true, Ordering::Relaxed);
            assert!(observer.metrics.try_recv().is_err());
            let start = Instant::now();
            drop(observer);
            assert!(start.elapsed() < Duration::from_secs(2));
            release.send(()).unwrap();
            server.join().unwrap();
        }
    }

    #[test]
    fn fifth_active_stream_is_rejected_without_forwarding() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let release = Arc::new(AtomicBool::new(false));
        let server_release = release.clone();
        let server = thread::spawn(move || {
            let mut workers = Vec::new();
            for _ in 0..4 {
                let (mut socket, _) = listener.accept().unwrap();
                let release = server_release.clone();
                workers.push(thread::spawn(move || {
                    read_request(&mut socket);
                    let token = "data: {\"choices\":[{\"delta\":{\"content\":\"started\"}}]}\n\n";
                    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{token}\r\n",token.len()).unwrap();
                    socket.flush().unwrap();
                    let deadline=Instant::now()+Duration::from_secs(5);
                    while !release.load(Ordering::Relaxed) && Instant::now()<deadline {thread::sleep(Duration::from_millis(10));}
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        let observer = proxy("LM Studio", &endpoint);
        let client = client();
        let mut responses = Vec::new();
        for _ in 0..4 {
            let response = client
                .post(format!("{}/v1/chat/completions", observer.endpoint))
                .json(&serde_json::json!({"model":"test","stream":true}))
                .send()
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            responses.push(response);
        }
        let fifth = client
            .post(format!("{}/v1/chat/completions", observer.endpoint))
            .json(&serde_json::json!({"model":"test","stream":true}))
            .send()
            .unwrap();
        assert_eq!(fifth.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(observer.metrics.try_recv().is_err());
        drop(responses);
        observer.stop.store(true, Ordering::Relaxed);
        drop(observer);
        release.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }

    #[test]
    fn metadata_forwarding_does_not_start_inference_or_create_metrics() {
        let (endpoint, requests, server) =
            upstream(vec![wire(200, "application/json", r#"{"models":[]}"#)]);
        let observer = proxy("Ollama", &endpoint);
        let response = client()
            .get(format!("{}/api/tags", observer.endpoint))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().unwrap(), r#"{"models":[]}"#);
        server.join().unwrap();
        let requests = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /api/tags "));
        assert!(observer.metrics.try_recv().is_err());
    }

    #[test]
    fn oversized_upstream_body_and_redirect_are_rejected_before_forwarding() {
        for wire in [
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",MAX_RESPONSE+1),
            "HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ] {
            let (endpoint, _, server)=upstream(vec![wire]);
            let observer=proxy("LM Studio",&endpoint);
            let response=client().post(format!("{}/v1/chat/completions",observer.endpoint))
                .json(&serde_json::json!({"model":"test","stream":true})).send().unwrap();
            assert_eq!(response.status(),StatusCode::BAD_GATEWAY);
            assert!(response.headers().get(header::LOCATION).is_none());
            assert!(!response.text().unwrap().contains("192.0.2.1"));
            server.join().unwrap();assert!(observer.metrics.try_recv().is_err());
        }
    }

    #[test]
    fn startup_auth_policy_cannot_remove_upstream_protection() {
        assert!(front_auth_policy(false, None).unwrap().is_none());
        assert!(
            front_auth_policy(false, Some(String::new()))
                .unwrap()
                .is_none()
        );
        for absent in [None, Some(String::new())] {
            let error = front_auth_policy(true, absent)
                .err()
                .expect("upstream auth needs frontend auth");
            assert!(error.to_string().contains("RUST_CPU_OBSERVER_TOKEN"));
        }
        for invalid in ["secret with space", "secret\r\nInjected: x", "é-token"] {
            let error = front_auth_policy(true, Some(invalid.into()))
                .err()
                .expect("invalid credential must fail");
            assert!(!error.to_string().contains(invalid));
        }
        assert!(front_auth_policy(true, Some("x".repeat(MAX_TOKEN + 1))).is_err());
        assert!(ProxyAuth::new(Some("upstream-test-key".into()), None).is_err());
        let token = front_auth_policy(true, Some("frontend-test-key".into()))
            .unwrap()
            .unwrap();
        assert!(constant_time_matches(&token, b"frontend-test-key"));
        for wrong in [
            b"".as_slice(),
            b"frontend-test-keY",
            b"frontend-test-key-extra",
            b"frontend-test-ke",
        ] {
            assert!(!constant_time_matches(&token, wrong));
        }
    }

    #[test]
    fn frontend_auth_returns_401_and_only_upstream_key_is_forwarded() {
        let backend_key = "distinct-backend-synthetic-key";
        let frontend_key = "distinct-frontend-synthetic-key";
        let result_body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"hello"}}],"usage":{"completion_tokens":3,"prompt_tokens":7}}"#;
        let (endpoint, requests, server) =
            upstream(vec![wire(200, "application/json", result_body)]);
        let auth = ProxyAuth::new(Some(backend_key.into()), Some(frontend_key.into())).unwrap();
        let observer = proxy_with_auth("LM Studio", &endpoint, auth);
        let client = client();
        for header_value in [
            None,
            Some("Bearer wrong-key".to_string()),
            Some(format!("Bearer {backend_key}")),
            Some(format!("Basic {frontend_key}")),
        ] {
            let mut request = client
                .post(format!("{}/v1/chat/completions", observer.endpoint))
                .json(&serde_json::json!({"model":"protected-model","stream":false}));
            if let Some(value) = header_value {
                request = request.header(header::AUTHORIZATION, value);
            }
            let response = request.send().unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
            let body = response.text().unwrap();
            assert!(!body.contains(backend_key));
            assert!(!body.contains(frontend_key));
        }
        let response = client
            .post(format!("{}/v1/chat/completions", observer.endpoint))
            .bearer_auth(frontend_key)
            .json(&serde_json::json!({"model":"protected-model","stream":false}))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().unwrap(), result_body);
        let metric = observer
            .metrics
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        let serialized = serde_json::to_string(&metric).unwrap();
        assert!(!serialized.contains(backend_key));
        assert!(!serialized.contains(frontend_key));
        server.join().unwrap();
        let requests = requests.try_iter().collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains(&format!("Bearer {backend_key}")));
        assert!(!requests[0].contains(frontend_key));
    }
}
