use crate::{catalog, model::*, App};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{collections::HashMap, convert::Infallible, sync::Arc, time::Duration};

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/models", get(models))
        .route("/v1/models/aliases", get(model_aliases))
        .route("/models", get(gemini_models))
        .route("/v1beta/models", get(gemini_models))
        .route("/v1/chat/completions", post(completions))
        .route("/v1/tasks", post(create_task))
        .route("/v1/tasks/{id}", get(get_task))
        .route("/v1/tasks/{id}/cancel", post(cancel_task))
        .route("/models/{action}", get(gemini_model).post(gemini_generate))
        .route(
            "/v1beta/models/{action}",
            get(gemini_model).post(gemini_generate),
        )
        .route("/media/{filename}", get(media))
        .route(
            "/api/browser-login/sync",
            post(sync_browser_login).options(cors_options),
        )
        .route(
            "/api/browser-bridge/pair",
            post(pair_browser).options(cors_options),
        )
        .route(
            "/api/browser-bridge/poll",
            post(poll_browser).options(cors_options),
        )
        .route(
            "/extension/flow2api-flow-login-sync.user.js",
            get(userscript_content),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .with_state(app)
}
struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(
                json!({"error":{"message":self.1,"type":"flow2api_error","code":self.0.as_u16()}}),
            ),
        )
            .into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(StatusCode::BAD_REQUEST, error.to_string())
    }
}
struct GeminiError(ApiError);
fn gemini_error(status: StatusCode, message: &str) -> Value {
    let name = match status.as_u16() {
        400 | 413 | 415 | 422 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        409 => "ABORTED",
        429 => "RESOURCE_EXHAUSTED",
        502 | 503 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => "INTERNAL",
    };
    json!({"error":{"code":status.as_u16(),"message":message,"status":name}})
}
impl IntoResponse for GeminiError {
    fn into_response(self) -> Response {
        (self.0 .0, Json(gemini_error(self.0 .0, &self.0 .1))).into_response()
    }
}
impl From<ApiError> for GeminiError {
    fn from(error: ApiError) -> Self {
        Self(error)
    }
}
impl From<anyhow::Error> for GeminiError {
    fn from(error: anyhow::Error) -> Self {
        Self(ApiError::from(error))
    }
}
fn authorize(
    app: &App,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
) -> Result<(), ApiError> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let key = bearer
        .or_else(|| headers.get("x-goog-api-key").and_then(|h| h.to_str().ok()))
        .or_else(|| query.get("key").map(String::as_str));
    if key != Some(app.store.settings()?.api_key.as_str()) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "API Key 不正确".into()));
    }
    Ok(())
}
async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    let s = app.status();
    Json(
        json!({"status":"healthy","backend_running":true,"service":s.service,"total_tokens":s.total_accounts,"enabled_accounts":s.accounts,"active_tokens":s.ready_accounts,"has_active_tokens":s.ready_accounts>0,"readiness_source":"verified_browser_session","captcha_method":"browser_session","remote_browser_configured":false,"tasks":{"running":s.running,"queued":s.queued,"succeeded":s.succeeded,"failed":s.failed}}),
    )
}
async fn metrics(State(app): State<Arc<App>>) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        crate::monitoring::render(&app.status()),
    )
        .into_response()
}
async fn models(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    Ok(Json(catalog::public_models()))
}
async fn model_aliases(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    Ok(Json(crate::resolver::public_aliases()))
}
fn gemini_resource(model: &Value) -> Value {
    json!({"name":format!("models/{}",model["id"].as_str().unwrap()),"displayName":model["id"],"description":model["description"],"version":"flow2api","inputTokenLimit":0,"outputTokenLimit":0,"supportedGenerationMethods":["generateContent","streamGenerateContent"],"capabilities":model["capabilities"]})
}
async fn gemini_models(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, GeminiError> {
    authorize(&app, &headers, &query)?;
    Ok(Json(
        json!({"models":catalog::public_models()["data"].as_array().unwrap().iter().map(gemini_resource).collect::<Vec<_>>()}),
    ))
}
async fn gemini_model(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, GeminiError> {
    authorize(&app, &headers, &query)?;
    catalog::public_models()["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == id)
        .map(|model| Json(gemini_resource(model)))
        .ok_or_else(|| GeminiError(ApiError(StatusCode::NOT_FOUND, "模型不存在".into())))
}
async fn completions(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    authorize(&app, &headers, &query)?;
    let mut request = crate::request::openai(&body)?;
    let settings = app.store.settings()?;
    crate::request::append_history(&mut request, &body, &settings).await?;
    crate::resolver::prepare(&mut request, &body, &settings).await?;
    let task = app.submit(request).await?;
    if body["stream"] == true {
        return Ok(openai_stream(app, task));
    }
    let task = wait_task(&app, &task.id).await?;
    if task.state != "succeeded" {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            task.error.unwrap_or_else(|| "任务失败".into()),
        ));
    }
    let content = markdown(&app, &task);
    Ok(Json(json!({"id":task.id,"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":task.request.model,"choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":"stop"}],"usage":{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}})).into_response())
}
fn base_address(app: &App) -> String {
    app.status().address.unwrap_or_else(|| {
        format!(
            "http://127.0.0.1:{}",
            app.store.settings().map(|s| s.port).unwrap_or(8000)
        )
    })
}
fn task_json(app: &App, task: &Task) -> Value {
    let address = base_address(app);
    let results: Vec<Value> = task
        .results
        .iter()
        .map(|m| {
            json!({
                "kind": m.kind,
                "filename": m.filename,
                "size": m.size,
                "url": format!("{address}/media/{}", m.filename),
                "download_url": format!("{address}/media/{}?download=true", m.filename),
                "media_id": m.media_id,
                "created_at": m.created_at,
            })
        })
        .collect();
    json!({
        "id": task.id,
        "object": "flow2api.task",
        "model": task.request.model,
        "state": task.state,
        "stage": task.stage,
        "created_at": task.created_at,
        "updated_at": task.updated_at,
        "error": task.error,
        "results": results,
    })
}
async fn create_task(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    let mut request = crate::request::openai(&body)?;
    let settings = app.store.settings()?;
    crate::request::append_history(&mut request, &body, &settings).await?;
    crate::resolver::prepare(&mut request, &body, &settings).await?;
    let task = app.submit(request).await?;
    if body["wait"] == true {
        let task = wait_task(&app, &task.id).await?;
        if task.state != "succeeded" {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                task.error.unwrap_or_else(|| "任务失败".into()),
            ));
        }
        return Ok(Json(task_json(&app, &task)));
    }
    Ok(Json(task_json(&app, &task)))
}
async fn get_task(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    let task = app
        .store
        .task(&id)
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "任务不存在".into()))?;
    Ok(Json(task_json(&app, &task)))
}
async fn cancel_task(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    let task = app.cancel_task(&id).await?;
    Ok(Json(task_json(&app, &task)))
}
async fn wait_task(app: &App, id: &str) -> Result<Task, ApiError> {
    loop {
        let task = app.store.task(id)?;
        if !["queued", "running"].contains(&task.state.as_str()) {
            return Ok(task);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}
fn markdown(app: &App, task: &Task) -> String {
    let address = base_address(app);
    task.results
        .iter()
        .map(|m| {
            if m.kind == "image" {
                format!("![生成图片]({address}/media/{})", m.filename)
            } else {
                format!(
                    "<video src=\"{address}/media/{}\" controls></video>\nmedia ID: {}",
                    m.filename, m.media_id
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn openai_stream(app: Arc<App>, task: Task) -> Response {
    let stream = async_stream::stream! {
        let mut last_stage = String::new();
        loop {
            match app.store.task(&task.id) {
                Ok(current) => {
                    if current.stage != last_stage {
                        last_stage = current.stage.clone();
                        yield Ok::<_,Infallible>(Event::default().data(json!({"id":task.id,"object":"chat.completion.chunk","model":task.request.model,"choices":[{"index":0,"delta":{"reasoning_content":format!("{}\n",current.stage)},"finish_reason":null}]}).to_string()));
                    }
                    if current.state == "succeeded" {
                        yield Ok(Event::default().data(json!({"id":task.id,"object":"chat.completion.chunk","model":task.request.model,"choices":[{"index":0,"delta":{"content":markdown(&app,&current)},"finish_reason":null}]}).to_string()));
                        yield Ok(Event::default().data(json!({"id":task.id,"object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}).to_string()));
                        yield Ok(Event::default().data("[DONE]")); break;
                    }
                    if !["queued","running"].contains(&current.state.as_str()) { yield Ok(Event::default().data(json!({"error":{"message":current.error.unwrap_or_else(||"任务未完成".into())}}).to_string())); break; }
                }
                Err(_) => { yield Ok(Event::default().data(json!({"error":{"message":"无法读取任务状态"}}).to_string())); break; }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
async fn gemini_result(app: &App, task: &Task) -> Result<Value, ApiError> {
    let mut parts = vec![];
    for media in &task.results {
        if media.kind == "image" {
            let bytes = tokio::fs::read(app.store.root.join("generated").join(&media.filename))
                .await
                .map_err(|_| {
                    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "作品文件无法读取".into())
                })?;
            parts.push(json!({"inlineData":{"mimeType":mime(&media.filename),"data":STANDARD.encode(bytes)}}));
        } else {
            parts.push(json!({"fileData":{"mimeType":mime(&media.filename),"fileUri":format!("{}/media/{}",app.status().address.unwrap_or_default(),media.filename)}}));
        }
    }
    Ok(
        json!({"candidates":[{"content":{"role":"model","parts":parts},"finishReason":"STOP","index":0}],"modelVersion":task.request.model}),
    )
}
async fn gemini_generate(
    State(app): State<Arc<App>>,
    Path(action): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, GeminiError> {
    authorize(&app, &headers, &query)?;
    let Json(body) =
        body.map_err(|error| ApiError(error.status(), "Gemini 请求 JSON 无效".into()))?;
    let (id, method) = action
        .rsplit_once(':')
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "未知 Gemini 方法".into()))?;
    if !["generateContent", "streamGenerateContent"].contains(&method) {
        return Err(ApiError(StatusCode::NOT_FOUND, "未知 Gemini 方法".into()).into());
    }
    let mut request = crate::request::gemini(id, &body)?;
    crate::resolver::prepare(&mut request, &body, &app.store.settings()?).await?;
    let task = app.submit(request).await?;
    if method == "streamGenerateContent" {
        let stream = async_stream::stream! {
            match wait_task(&app,&task.id).await {
                Ok(task) if task.state == "succeeded" => match gemini_result(&app,&task).await { Ok(value)=>yield Ok::<_,Infallible>(Event::default().data(value.to_string())), Err(error)=>yield Ok(Event::default().data(gemini_error(error.0, &error.1).to_string())) },
                Ok(task)=>yield Ok(Event::default().data(gemini_error(StatusCode::BAD_GATEWAY, task.error.as_deref().unwrap_or("任务失败")).to_string())),
                Err(error)=>yield Ok(Event::default().data(gemini_error(error.0, &error.1).to_string())),
            }
        };
        return Ok(Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response());
    }
    let task = wait_task(&app, &task.id).await?;
    if task.state != "succeeded" {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            task.error.unwrap_or_else(|| "任务失败".into()),
        )
        .into());
    }
    Ok(Json(gemini_result(&app, &task).await?).into_response())
}
fn mime(filename: &str) -> &'static str {
    match filename.rsplit('.').next().unwrap_or_default() {
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "webm" => "video/webm",
        _ => "video/mp4",
    }
}
async fn media(
    State(app): State<Arc<App>>,
    Path(filename): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let (hash, extension) = filename
        .split_once('.')
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "文件不存在".into()))?;
    if hash.len() != 64
        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        || !["jpg", "png", "webp", "gif", "mp4", "webm"].contains(&extension)
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "文件不存在".into()));
    }
    let mut file = tokio::fs::File::open(app.store.root.join("generated").join(&filename))
        .await
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "文件不存在".into()))?;
    let size = file
        .metadata()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "读取作品失败".into()))?
        .len();
    let range = match headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        Some(raw) => match byte_range(raw, size) {
            Some(range) => Some(range),
            None => {
                let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes */{size}").parse().unwrap(),
                );
                return Ok(response);
            }
        },
        None => None,
    };
    let (start, end) = range.unwrap_or((0, size.saturating_sub(1)));
    let length = if size == 0 { 0 } else { end - start + 1 };
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "读取作品失败".into()))?;
    let stream = async_stream::stream! {
        let mut remaining=length;
        while remaining>0 {
            let mut buffer=vec![0;remaining.min(64*1024) as usize];
            let count=match file.read(&mut buffer).await {
                Ok(0)=>{yield Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof,"media file truncated"));break;},
                Ok(count)=>count,
                Err(error)=>{yield Err(error);break;},
            };
            buffer.truncate(count);remaining-=count as u64;
            yield Ok(buffer);
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, length.to_string().parse().unwrap());
    if range.is_some() {
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{size}").parse().unwrap(),
        );
    }
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, mime(&filename).parse().unwrap());
    if query.get("download").is_some_and(|v| v == "true") {
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\"")
                .parse()
                .unwrap(),
        );
    }
    Ok(response)
}

fn byte_range(raw: &str, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let (start, end) = raw.strip_prefix("bytes=")?.split_once('-')?;
    if start.is_empty() {
        let count = end.parse::<u64>().ok()?;
        if count == 0 {
            return None;
        }
        return Some((size.saturating_sub(count), size - 1));
    }
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().ok()?.min(size - 1)
    };
    if start > end || start >= size {
        None
    } else {
        Some((start, end))
    }
}

async fn cors_options() -> Response {
    (
        StatusCode::NO_CONTENT,
        [
            ("access-control-allow-origin", "*"),
            ("access-control-allow-methods", "GET, POST, OPTIONS"),
            ("access-control-allow-headers", "*"),
        ],
    )
        .into_response()
}

async fn userscript_content() -> impl IntoResponse {
    let script = include_str!("../assets/extension/flow2api-flow-login-sync.user.js");
    (
        StatusCode::OK,
        [
            ("content-type", "text/javascript; charset=utf-8"),
            (
                "content-disposition",
                "inline; filename=\"flow2api-flow-login-sync.user.js\"",
            ),
            ("access-control-allow-origin", "*"),
        ],
        script,
    )
}

async fn sync_browser_login(
    State(_app): State<Arc<App>>,
    Json(_payload): Json<Value>,
) -> Result<Response, ApiError> {
    Err(ApiError(StatusCode::GONE, "旧版 Cookie 搬运已停用。请在应用安装新版连接脚本、获取连接码，再在当前 Flow 标签页连接。支持 Ego Lite 和 Chrome。".into()))
}

#[derive(serde::Deserialize)]
struct PairBrowser {
    #[serde(default)]
    code: String,
    email: String,
}
async fn pair_browser(
    State(app): State<Arc<App>>,
    Json(payload): Json<PairBrowser>,
) -> Result<Json<Value>, ApiError> {
    let (id, token) = app.pair_browser(&payload.code, &payload.email).await?;
    Ok(Json(json!({"account_id":id,"token":token})))
}
#[derive(serde::Deserialize)]
struct BrowserPoll {
    #[serde(default)]
    result: Option<crate::bridge::Completion>,
    #[serde(default)]
    heartbeat: bool,
    #[serde(default)]
    resume: bool,
}
async fn poll_browser(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(payload): Json<BrowserPoll>,
) -> Result<Json<Value>, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "请重新连接浏览器".into()))?;
    let peer = app.browser_peer(token).map_err(|_| {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "浏览器连接已失效，请重新连接".into(),
        )
    })?;
    if payload.resume || !peer.online() {
        peer.revalidate();
    }
    peer.touch();
    let commands = if payload.heartbeat {
        vec![]
    } else {
        peer.poll(payload.result).await
    };
    if !peer
        .validating
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        let app = app.clone();
        let peer = peer.clone();
        tokio::spawn(async move {
            app.validate_browser(peer).await;
        });
    }
    app.refresh()?;
    Ok(Json(
        json!({"commands":commands,"connection":peer.status()}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn client_discovery_routes_and_gemini_errors_match_protocol() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let key = app.store.settings()?.api_key;
        for (path, auth, status, field) in [
            ("/v1/models/aliases", true, 200, "data"),
            (
                "/models/gemini-3.1-flash-image-square",
                true,
                200,
                "supportedGenerationMethods",
            ),
            (
                "/v1beta/models/gemini-3.1-flash-image-square",
                true,
                200,
                "supportedGenerationMethods",
            ),
            ("/v1beta/models/not-a-model", true, 404, "error"),
            ("/models", false, 401, "error"),
        ] {
            let mut request = axum::http::Request::builder().uri(path);
            if auth {
                request = request.header("x-goog-api-key", &key);
            }
            let response = router(app.clone())
                .oneshot(request.body(Body::empty())?)
                .await?;
            assert_eq!(response.status().as_u16(), status, "{path}");
            let body: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 1_000_000).await?,
            )?;
            assert!(!body[field].is_null(), "{path}");
            if status == 404 {
                assert_eq!(body["error"]["status"], "NOT_FOUND");
            }
            if status == 401 {
                assert_eq!(body["error"]["status"], "UNAUTHENTICATED");
            }
            if path.ends_with("aliases") {
                assert!(body["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["id"] == "gemini-3.1-flash-image" && m["is_alias"] == true));
            }
        }
        for input in ["{", "{\"contents\":[]}", "{\"contents\":[{\"parts\":[{\"inlineData\":{\"mimeType\":\"video/mp4\",\"data\":\"bad\"}}]}]}"] {
            let request = axum::http::Request::builder().method("POST").uri("/models/gemini-3.1-flash-image:generateContent")
                .header("content-type", "application/json").header("x-goog-api-key", &key).body(Body::from(input))?;
            let response = router(app.clone()).oneshot(request).await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body: Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1_000_000).await?)?;
            assert_eq!(body["error"]["code"], 400);
            assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
        }
        assert!(app.store.tasks()?.is_empty());
        Ok(())
    }
    #[tokio::test]
    async fn media_ranges_support_video_seeking_and_reject_invalid_ranges() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let filename = format!("{}.mp4", "a".repeat(64));
        tokio::fs::write(
            directory.path().join("generated").join(&filename),
            b"0123456789",
        )
        .await?;
        for (range, expected, expected_body) in [
            ("bytes=2-5", 206, "2345"),
            ("bytes=-3", 206, "789"),
            ("bytes=8-", 206, "89"),
            ("bytes=20-30", 416, ""),
        ] {
            let request = axum::http::Request::builder()
                .uri(format!("/media/{filename}"))
                .header(header::RANGE, range)
                .body(Body::empty())?;
            let response = router(app.clone()).oneshot(request).await?;
            assert_eq!(response.status().as_u16(), expected);
            let body = axum::body::to_bytes(response.into_body(), 100).await?;
            assert_eq!(body.as_ref(), expected_body.as_bytes());
        }
        Ok(())
    }
    #[tokio::test]
    async fn live_listener_state_auth_and_port_conflict() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let mut settings = app.store.settings()?;
        settings.port = occupied.local_addr()?.port();
        app.save_settings(settings).await?;
        assert!(app.start_service().await.is_err());
        assert_eq!(app.status().service, "failed");
        assert!(app.status().last_error.is_some());
        drop(occupied);
        app.start_service().await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let address = app.status().address.unwrap();
        let metrics = client.get(format!("{address}/metrics")).send().await?;
        assert_eq!(metrics.status(), 200);
        assert!(metrics.text().await?.contains("flow2api_up 1\n"));
        assert_eq!(
            client
                .get(format!("{address}/health"))
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            client
                .get(format!("{address}/v1/models"))
                .send()
                .await?
                .status(),
            401
        );
        let response = client
            .get(format!("{address}/v1/models"))
            .bearer_auth(app.store.settings()?.api_key)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert!(
            response.json::<Value>().await?["data"]
                .as_array()
                .unwrap()
                .len()
                > 30
        );
        // Saving settings while idle and running automatically restarts on the new port.
        let next_port = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let mut updated = app.store.settings()?;
        updated.port = next_port.local_addr()?.port();
        drop(next_port);
        app.save_settings(updated.clone()).await?;
        let address = app.status().address.unwrap();
        assert!(address.ends_with(&updated.port.to_string()));
        assert_eq!(
            client
                .get(format!("{address}/health"))
                .send()
                .await?
                .status(),
            200
        );
        // Ordinary API stop is reversible; application shutdown is terminal.
        app.stop_service().await?;
        app.start_service().await?;
        let mut pending = Task {
            id: uuid::Uuid::new_v4().to_string(),
            request: GenerationRequest {
                model: "gemini-3.1-flash-image-square".into(),
                prompt: "pending fixture".into(),
                account_id: None,
                images: vec![],
                video_id: None,
            },
            state: "queued".into(),
            stage: "等待执行".into(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![],
        };
        app.store.save_task(&pending)?;
        app.refresh()?;
        assert!(app.shutdown().await.is_err());
        assert_eq!(app.status().service, "running");
        assert_eq!(
            client
                .get(format!("{address}/health"))
                .send()
                .await?
                .status(),
            200
        );
        pending.state = "interrupted".into();
        app.store.save_task(&pending)?;
        app.refresh()?;
        app.shutdown().await?;
        assert!(app
            .start_service()
            .await
            .unwrap_err()
            .to_string()
            .contains("正在退出"));
        assert!(app
            .open_login("unused")
            .await
            .unwrap_err()
            .to_string()
            .contains("正在退出"));
        assert!(app
            .sync_account("unused")
            .await
            .unwrap_err()
            .to_string()
            .contains("正在退出"));
        assert!(app.save_settings(app.store.settings()?).await.is_err());
        // Repeated shutdown requests remain harmless.
        app.shutdown().await?;
        assert_eq!(app.status().service, "stopped");
        // A failed updater installation restores admission and the API, without
        // changing credentials or resubmitting interrupted tasks.
        let before = app.store.settings()?;
        app.resume_after_failed_update(true).await?;
        assert_eq!(app.status().service, "running");
        assert_eq!(app.store.settings()?.api_key, before.api_key);
        assert_eq!(
            client
                .get(format!("{address}/health"))
                .send()
                .await?
                .status(),
            200
        );
        app.shutdown().await?;
        assert!(
            tokio::net::TcpStream::connect(address.trim_start_matches("http://"))
                .await
                .is_err()
        );
        Ok(())
    }
    #[tokio::test]
    async fn missing_account_does_not_create_fake_success_or_task() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let reserved = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let mut settings = app.store.settings()?;
        settings.port = reserved.local_addr()?.port();
        app.save_settings(settings).await?;
        drop(reserved);
        app.start_service().await?;
        let key = app.store.settings()?.api_key;
        let request=axum::http::Request::builder().uri("/v1/chat/completions").method("POST").header("authorization",format!("Bearer {key}")).header("content-type","application/json").body(Body::from(json!({"model":"gemini-3.1-flash-image-square","messages":[{"role":"user","content":"hello"}]}).to_string()))?;
        let response = router(app.clone()).oneshot(request).await?;
        assert_eq!(response.status(), 400);
        assert!(app.store.tasks()?.is_empty());
        let body = axum::body::to_bytes(response.into_body(), 1_000_000).await?;
        assert!(String::from_utf8_lossy(&body).contains("没有已启用且完成登录同步的账号"));
        app.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn stream_failure_is_error_without_done_marker() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let task = Task {
            id: "failed-task".into(),
            request: GenerationRequest {
                model: "gemini-3.1-flash-image-square".into(),
                prompt: "prompt".into(),
                account_id: None,
                images: vec![],
                video_id: None,
            },
            state: "failed".into(),
            stage: "failed".into(),
            created_at: now(),
            updated_at: now(),
            error: Some("upstream rejected".into()),
            results: vec![],
        };
        for state in ["failed", "cancelled"] {
            let mut terminal = task.clone();
            terminal.state = state.into();
            terminal.error = Some(
                if state == "cancelled" {
                    "任务已在本机取消，未提交到 Flow"
                } else {
                    "upstream rejected"
                }
                .into(),
            );
            app.store.save_task(&terminal)?;
            let response = openai_stream(app.clone(), terminal.clone());
            assert_eq!(response.status(), 200);
            let body = axum::body::to_bytes(response.into_body(), 1_000_000).await?;
            let text = String::from_utf8(body.to_vec())?;
            assert!(text.contains(terminal.error.as_deref().unwrap()));
            assert!(!text.contains("[DONE]"));
            assert!(!text.contains("\"finish_reason\":\"stop\""));
            assert_eq!(
                wait_task(&app, &terminal.id)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.1))?
                    .state,
                state
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn browser_login_sync_routes_and_cors_work() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;

        // 1. OPTIONS CORS preflight
        let options_req = axum::http::Request::builder()
            .method("OPTIONS")
            .uri("/api/browser-login/sync")
            .body(Body::empty())?;
        let resp: Response = router(app.clone()).oneshot(options_req).await?;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );

        // 2. Userscript served
        let userscript_req = axum::http::Request::builder()
            .method("GET")
            .uri("/extension/flow2api-flow-login-sync.user.js")
            .body(Body::empty())?;
        let resp: Response = router(app.clone()).oneshot(userscript_req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 100_000).await?;
        let text = String::from_utf8(body.to_vec())?;
        assert!(text.contains("Flow2API 登录态同步"));

        // 3. Old cookie transfer is explicitly retired, never falsely accepted.
        let post_req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/browser-login/sync")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"login_account":"","google_cookies":[]}"#))?;
        let resp: Response = router(app.clone()).oneshot(post_req).await?;
        assert_eq!(resp.status(), StatusCode::GONE);

        Ok(())
    }

    #[tokio::test]
    async fn task_routes_support_lookup_cancel_and_auth() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let key = app.store.settings()?.api_key;
        let filename = format!("{}.mp4", "b".repeat(64));
        let queued = Task {
            id: "task-queued".into(),
            request: GenerationRequest {
                model: "veo-3.1-fast-8s-landscape".into(),
                prompt: "ocean waves".into(),
                account_id: None,
                images: vec![],
                video_id: None,
            },
            state: "queued".into(),
            stage: "等待执行".into(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![Media {
                filename: filename.clone(),
                kind: "video".into(),
                size: 1024,
                created_at: now(),
                task_id: "task-queued".into(),
                media_id: "media-123".into(),
                prompt: "ocean waves".into(),
                model: "veo-3.1-fast-8s-landscape".into(),
            }],
        };
        app.store.save_task(&queued)?;

        let unauth = axum::http::Request::builder()
            .uri("/v1/tasks/task-queued")
            .body(Body::empty())?;
        assert_eq!(
            router(app.clone()).oneshot(unauth).await?.status(),
            StatusCode::UNAUTHORIZED
        );

        let missing = axum::http::Request::builder()
            .uri("/v1/tasks/no-such-task")
            .header("authorization", format!("Bearer {key}"))
            .body(Body::empty())?;
        assert_eq!(
            router(app.clone()).oneshot(missing).await?.status(),
            StatusCode::NOT_FOUND
        );

        let lookup = axum::http::Request::builder()
            .uri("/v1/tasks/task-queued")
            .header("authorization", format!("Bearer {key}"))
            .body(Body::empty())?;
        let resp = router(app.clone()).oneshot(lookup).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1_000_000).await?)?;
        assert_eq!(body["id"], "task-queued");
        assert_eq!(body["state"], "queued");
        assert_eq!(body["results"][0]["media_id"], "media-123");
        assert!(body["results"][0]["download_url"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/media/{filename}?download=true")));

        let cancel = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/tasks/task-queued/cancel")
            .header("authorization", format!("Bearer {key}"))
            .body(Body::empty())?;
        let resp = router(app.clone()).oneshot(cancel).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1_000_000).await?)?;
        assert_eq!(body["state"], "cancelled");
        Ok(())
    }
}
