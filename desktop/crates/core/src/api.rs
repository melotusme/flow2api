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
        .route("/v1/models", get(models))
        .route("/models", get(gemini_models))
        .route("/v1beta/models", get(gemini_models))
        .route("/v1/chat/completions", post(completions))
        .route("/models/{action}", post(gemini_generate))
        .route("/v1beta/models/{action}", post(gemini_generate))
        .route("/media/{filename}", get(media))
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
        json!({"status":"healthy","service":s.service,"active_tokens":s.accounts,"tasks":{"running":s.running,"queued":s.queued,"succeeded":s.succeeded,"failed":s.failed}}),
    )
}
async fn models(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    Ok(Json(catalog::public_models()))
}
async fn gemini_models(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    authorize(&app, &headers, &query)?;
    Ok(Json(
        json!({"models":catalog::public_models()["data"].as_array().unwrap().iter().map(|m|json!({"name":format!("models/{}",m["id"].as_str().unwrap()),"displayName":m["id"],"supportedGenerationMethods":["generateContent","streamGenerateContent"],"capabilities":m["capabilities"]})).collect::<Vec<_>>()}),
    ))
}
fn parse_openai(body: &Value) -> Result<GenerationRequest, ApiError> {
    let model = body["model"]
        .as_str()
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "缺少 model".into()))?
        .to_owned();
    let mut prompt = vec![];
    let mut images = vec![];
    let mut video_id = None;
    for message in body["messages"]
        .as_array()
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "缺少 messages".into()))?
    {
        if let Some(text) = message["content"].as_str() {
            prompt.push(text.to_owned());
        }
        for part in message["content"].as_array().into_iter().flatten() {
            if let Some(text) = part["text"].as_str() {
                prompt.push(text.to_owned());
            }
            if let Some(url) = part["image_url"]["url"].as_str() {
                if let Some(id) = url.strip_prefix("extend://") {
                    video_id = Some(id.into());
                } else {
                    images.push(url.into());
                }
            }
        }
    }
    Ok(GenerationRequest {
        model,
        prompt: prompt.join("\n"),
        images,
        account_id: None,
        video_id,
    })
}
async fn completions(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    authorize(&app, &headers, &query)?;
    let mut request = parse_openai(&body)?;
    request.model = resolve_model(&request.model, &body);
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
fn resolve_model(id: &str, body: &Value) -> String {
    crate::resolver::resolve(id, body)
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
    let address = app.status().address.unwrap_or_else(|| {
        format!(
            "http://127.0.0.1:{}",
            app.store.settings().map(|s| s.port).unwrap_or(8000)
        )
    });
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
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    authorize(&app, &headers, &query)?;
    let (id, method) = action
        .rsplit_once(':')
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "未知 Gemini 方法".into()))?;
    if !["generateContent", "streamGenerateContent"].contains(&method) {
        return Err(ApiError(StatusCode::NOT_FOUND, "未知 Gemini 方法".into()));
    }
    let mut texts = vec![];
    let mut images = vec![];
    for part in body["systemInstruction"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(text) = part["text"].as_str() {
            texts.push(text.to_owned());
        }
    }
    for content in body["contents"].as_array().into_iter().flatten() {
        for part in content["parts"].as_array().into_iter().flatten() {
            if let Some(text) = part["text"].as_str() {
                texts.push(text.to_owned());
            }
            if let Some(data) = part["inlineData"]["data"].as_str() {
                images.push(format!(
                    "data:{};base64,{data}",
                    part["inlineData"]["mimeType"]
                        .as_str()
                        .unwrap_or("image/jpeg")
                ));
            }
            if let Some(url) = part["fileData"]["fileUri"].as_str() {
                images.push(url.into());
            }
        }
    }
    let task = app
        .submit(GenerationRequest {
            model: resolve_model(id, &body),
            prompt: texts.join("\n"),
            images,
            account_id: None,
            video_id: None,
        })
        .await?;
    if method == "streamGenerateContent" {
        let stream = async_stream::stream! {
            match wait_task(&app,&task.id).await {
                Ok(task) if task.state == "succeeded" => match gemini_result(&app,&task).await { Ok(value)=>yield Ok::<_,Infallible>(Event::default().data(value.to_string())), Err(error)=>yield Ok(Event::default().data(json!({"error":{"message":error.1}}).to_string())) },
                Ok(task)=>yield Ok(Event::default().data(json!({"error":{"message":task.error}}).to_string())),
                Err(error)=>yield Ok(Event::default().data(json!({"error":{"message":error.1}}).to_string())),
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
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
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
        app.shutdown().await?;
        assert_eq!(app.status().service, "stopped");
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
        app.store.save_task(&task)?;
        let response = openai_stream(app, task);
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1_000_000).await?;
        let text = String::from_utf8(body.to_vec())?;
        assert!(text.contains("upstream rejected"));
        assert!(!text.contains("[DONE]"));
        assert!(!text.contains("\"finish_reason\":\"stop\""));
        Ok(())
    }
}
