//! Phase 10: Web server — `lun serve`.

use std::path::PathBuf;
use std::sync::Arc;

use crate::Task;
use axum::body::Bytes;
use axum::extract::{Path, Query, State, Request};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::db::{Lun, TaskSpec, TaskUpdateSpec, TaskListSpec, TaskSort, Attachment};

static WEB_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

pub struct ServerState {
    pub lun: Arc<Mutex<Lun>>,
    pub data_dir: PathBuf,
}

pub async fn serve(lun: Lun, data_dir: PathBuf, bind: &str) -> anyhow::Result<()> {
    let state = Arc::new(ServerState {
        lun: Arc::new(Mutex::new(lun)),
        data_dir,
    });

    let app = Router::new()
        .route("/api/projects", get(list_projects))
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/search", get(search_tasks))
        .route("/api/tasks/{key}", get(task_by_key))
        .route("/api/tasks/{key}", post(update_task))
        .route("/api/tasks/{key}/logs", get(task_logs))
        .route("/api/tasks/{key}/attachments", get(task_attachments))
        .route("/api/tasks/{key}/attachments", post(upload_attachment))
        .route("/api/tasks/{key}/links", get(task_links))
        .route("/api/tasks/{key}/prs", get(task_prs))
        .route("/api/scopes", get(list_scopes))
        .route("/api/markdown/preview", post(markdown_preview))
        .route("/api/attach/{id}", get(serve_attachment))
        .route("/manifest.json", get(serve_manifest))
        .route("/favicon.ico", get(serve_favicon))
        .fallback(static_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("lun serve: listening on {}", bind);
    axum::serve(listener, app).await.map_err(Into::into)
}

fn api_err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn list_projects(state: State<Arc<ServerState>>) -> Response {
    let lun = state.lun.lock().await;
    match lun.list_all_projects() {
        Ok(p) => Json(p).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct TaskQueryParams {
    #[serde(default)] project_id: Option<i64>,
    #[serde(default)] status: Option<String>,
    #[serde(default)] priority: Option<String>,
    #[serde(default)] assignee: Option<String>,
    #[serde(default)] scope: Option<String>,
    #[serde(default)] archived: Option<bool>,
    #[serde(default)] sort: Option<String>,
}

async fn list_tasks(state: State<Arc<ServerState>>, Query(params): Query<TaskQueryParams>) -> Response {
    let lun = state.lun.lock().await;
    let spec = TaskListSpec {
        project_id: params.project_id,
        status: params.status,
        priority: params.priority,
        assignee: params.assignee,
        scope: params.scope,
        include_archived: params.archived.unwrap_or(false),
        sort: match params.sort.as_deref() {
            Some("key") => TaskSort::Key, Some("title") => TaskSort::Title,
            Some("status") => TaskSort::Status, Some("priority") => TaskSort::Priority,
            Some("updated") => TaskSort::Updated, _ => TaskSort::Key,
        },
    };
    match lun.list_tasks_with(&spec) {
        Ok(t) => Json(t).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct SearchParams { q: String }

async fn search_tasks(state: State<Arc<ServerState>>, Query(params): Query<SearchParams>) -> Response {
    if params.q.is_empty() {
        return Json::<Vec<Task>>(vec![]).into_response();
    }
    let lun = state.lun.lock().await;
    let all = match lun.list_tasks_with(&TaskListSpec {
        project_id: None, status: None, priority: None,
        assignee: None, scope: None, include_archived: false, sort: TaskSort::Key,
    }) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let ql = params.q.to_lowercase();
    let matched: Vec<_> = all.into_iter().filter(|t| {
        t.title.to_lowercase().contains(&ql)
            || t.task_key.to_lowercase().contains(&ql)
            || t.notes.to_lowercase().contains(&ql)
    }).collect();
    Json(matched).into_response()
}

#[derive(Deserialize)]
struct CreateTaskRequest {
    title: String,
    #[serde(default)] project_id: Option<i64>,
    #[serde(default)] status: Option<String>,
    #[serde(default)] priority: Option<String>,
    #[serde(default)] assignee: Option<String>,
    #[serde(default)] branch: Option<String>,
    #[serde(default)] scope: Option<String>,
    #[serde(default)] labels: Option<String>,
    #[serde(default)] message: Option<String>,
}

async fn create_task(state: State<Arc<ServerState>>, Json(body): Json<CreateTaskRequest>) -> Response {
    let lun = state.lun.lock().await;
    let spec = TaskSpec {
        title: body.title, project: body.project_id, status: body.status,
        priority: body.priority, assignee: body.assignee, branch: body.branch,
        scope: body.scope, labels: body.labels, message: body.message,
        user: Some("web".to_string()),
    };
    match lun.create_task(spec) {
        Ok(t) => (StatusCode::CREATED, Json(t)).into_response(),
        Err(e) => api_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn task_by_key(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    match lun.task_by_key(&key) {
        Ok(t) => Json(t).into_response(),
        Err(e) => api_err(StatusCode::NOT_FOUND, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct UpdateTaskRequest {
    #[serde(default)] title: Option<String>,
    #[serde(default)] project_id: Option<i64>,
    #[serde(default)] status: Option<String>,
    #[serde(default)] priority: Option<String>,
    #[serde(default)] assignee: Option<Option<String>>,
    #[serde(default)] branch: Option<Option<String>>,
    #[serde(default)] scope: Option<Option<String>>,
    #[serde(default)] labels: Option<String>,
    #[serde(default)] notes: Option<String>,
    #[serde(default)] message: Option<String>,
}

async fn update_task(state: State<Arc<ServerState>>, Path(key): Path<String>, Json(body): Json<UpdateTaskRequest>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    let spec = TaskUpdateSpec {
        title: body.title, project_id: body.project_id, status: body.status,
        priority: body.priority, assignee: body.assignee, branch: body.branch,
        scope: body.scope, labels: body.labels, notes: body.notes,
        message: body.message, user: Some("web".to_string()),
    };
    match lun.update_task(task.id, spec) {
        Ok(t) => Json(t).into_response(),
        Err(e) => api_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn task_logs(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.task_logs(task.id) {
        Ok(l) => Json(l).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn task_attachments(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.task_attachments(task.id) {
        Ok(a) => Json(a).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn upload_attachment(state: State<Arc<ServerState>>, Path(key): Path<String>, mut multipart: axum::extract::Multipart) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    let data_dir = &state.data_dir;
    let mut results = Vec::new();
    while let Some(field) = multipart.next_field().await.unwrap() {
        let filename = match field.file_name() { Some(n) => String::from(n), None => continue };
        let data = match field.bytes().await { Ok(d) => d, Err(_) => continue };
        let attach_dir = data_dir.join("attachments");
        std::fs::create_dir_all(&attach_dir).ok();
        let stored_path = attach_dir.join(&filename);
        if std::fs::write(&stored_path, &data).is_err() { continue; }
        let sps = stored_path.to_string_lossy().to_string();
        if let Ok(id) = lun.add_attachment(task.id, &filename, &sps, Some("web upload"), Some("web")) {
            results.push(Attachment { id, task_id: Some(task.id), project_id: None, filename, stored_path: sps, created_at: crate::db::Lun::now() });
        }
    }
    Json(results).into_response()
}

async fn serve_attachment(state: State<Arc<ServerState>>, Path(id): Path<i64>) -> Response {
    let lun = state.lun.lock().await;
    let attach = match lun.attachment_by_id(id) {
        Ok(a) => a, Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let path = PathBuf::from(&attach.stored_path);
    if !path.exists() { return StatusCode::NOT_FOUND.into_response(); }
    let data = match tokio::fs::read(&path).await {
        Ok(d) => d, Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let mime = from_path(&attach.filename).first_or_octet_stream();
    let mime_s = mime.to_string();
    (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, mime_s.as_str())], Bytes::from(data)).into_response()
}

async fn task_links(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.task_links(task.id) {
        Ok(l) => Json(l).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn task_prs(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    let mut prs = match lun.task_prs(task.id) {
        Ok(p) => p, Err(e) => return api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    prs.sort_by(|a, b| b.id.cmp(&a.id));
    Json(prs).into_response()
}

async fn list_scopes(state: State<Arc<ServerState>>) -> Response {
    let lun = state.lun.lock().await;
    match lun.list_all_scopes() {
        Ok(s) => Json(s).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct MarkdownPreviewRequest { #[serde(default)] text: String }

async fn markdown_preview(Json(body): Json<MarkdownPreviewRequest>) -> Response {
    let html = render_markdown(&body.text);
    Json(serde_json::json!({"html": html})).into_response()
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

pub fn render_markdown(text: &str) -> String {
    let text = resolve_wikilinks(text);
    let parser = pulldown_cmark::Parser::new_ext(&text, pulldown_cmark::Options::all());
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    use std::collections::HashSet;
    let mut builder = ammonia::Builder::new();
    let mut schemes = HashSet::new();
    for s in ["http", "https", "ftp", "mailto", "obsidian", "file"] {
        schemes.insert(s);
    }
    builder.url_schemes(schemes);
    // Allow `class` on all elements so KaTeX classes survive sanitization
    {
        let mut tag_attrs = builder.clone_tag_attributes();
        tag_attrs.entry("span").or_insert_with(HashSet::new).insert("class");
        for tags in tag_attrs.values_mut() {
            tags.insert("class");
        }
        builder.tag_attributes(tag_attrs);
    }
    let mut html = builder.clean(&html).to_string();
    // Rename pulldown-cmark's `math` classes to KaTeX-compatible names
    html = html.replace("class=\"math math-inline\"", "class=\"katex\"");
    html = html.replace("class=\"math math-display\"", "class=\"katex-display\"");
    html
}

fn resolve_wikilinks(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' && chars.peek() == Some(&'[') {
            chars.next();
            let mut link = String::new();
            loop {
                match chars.next() {
                    Some(']') if chars.peek() == Some(&']') => { chars.next(); break; }
                    Some(ch) => link.push(ch),
                    None => break,
                }
            }
            let encoded = urlencoding::encode(&link);
            result.push_str(&format!("[{link}](obsidian://open?vault=personal&file={})", encoded));
        } else {
            result.push(c);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Static files
// ---------------------------------------------------------------------------

async fn serve_manifest() -> Response {
    if let Some(file) = WEB_DIR.get_file("manifest.json") {
        (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "application/manifest+json")], Bytes::from(file.contents().to_vec())).into_response()
    } else {
        (StatusCode::NOT_FOUND, [(axum::http::header::CONTENT_TYPE, "application/json")], Bytes::from(b"{}" as &[u8])).into_response()
    }
}

async fn serve_favicon() -> Response {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><rect width=\"100\" height=\"100\" rx=\"20\" fill=\"#6c3baa\"/><text x=\"50\" y=\"68\" font-family=\"monospace\" font-size=\"60\" font-weight=\"bold\" fill=\"#ffffff\" text-anchor=\"middle\">l</text></svg>";
    (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "image/svg+xml")], Bytes::from(svg.as_bytes())).into_response()
}

async fn static_handler(req: Request) -> Response {
    let file_path = match req.uri().path().trim_start_matches('/') {
        "" | "/" => "index.html",
        p => p,
    };
    let entry = match WEB_DIR.get_file(file_path) {
        Some(f) => f,
        None => match WEB_DIR.get_file("index.html") {
            Some(f) => f,
            None => return (StatusCode::NOT_FOUND, [(axum::http::header::CONTENT_TYPE, "text/plain")], Bytes::from_static(b"Not Found")).into_response(),
        },
    };
    let content = entry.contents();
    let mime = from_path(file_path).first_or_octet_stream();
    let mime_s = mime.to_string();
    let cors = HeaderValue::from_static("*");
    let mut resp = (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, mime_s.as_str())], Bytes::from(content.to_vec())).into_response();
    resp.headers_mut().insert(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, cors);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_preserves_katex_class() {
        let html = render_markdown("some $x^2$ math");
        eprintln!("katex output: {}", html);
        assert!(html.contains("katex"), "katex class should be preserved in output: {}", html);
    }

    #[test]
    fn test_render_markdown_wikilinks() {
        let html = render_markdown("[[my note]]");
        eprintln!("wikilink output: {}", html);
        assert!(html.contains("obsidian://open?vault=personal"),
            "wikilink should resolve to obsidian:// URL: {}", html);
    }

    #[test]
    fn test_render_markdown_block_math() {
        let html = render_markdown("$$x^2 + y^2$$");
        eprintln!("block math: {}", html);
        assert!(html.contains("katex-display") || html.contains("katex"),
            "block math should have katex class: {}", html);
    }
}
