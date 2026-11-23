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

use crate::db::{LinkTarget, Lun, TaskSpec, TaskUpdateSpec, TaskListSpec, TaskSort, Attachment, ProjectSpec};

use pulldown_cmark::{Event, Options as MdOptions, Tag};

static WEB_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

pub struct ServerState {
    pub lun: Arc<Mutex<Lun>>,
    pub data_dir: PathBuf,
    /// The board directory (parent of `.lun/`), full path and `~`-form.
    pub board_root: String,
    pub board_display: String,
}

pub async fn serve(lun: Lun, data_dir: PathBuf, bind: &str) -> anyhow::Result<()> {
    let board_root = data_dir.parent().map(PathBuf::from).unwrap_or_else(|| data_dir.clone());
    let state = Arc::new(ServerState {
        lun: Arc::new(Mutex::new(lun)),
        board_display: crate::root::display_path(&board_root, crate::root::home_dir().as_deref()),
        board_root: board_root.display().to_string(),
        data_dir,
    });

    let app = Router::new()
        .route("/api/board", get(board_info))
        .route("/api/projects", get(list_projects))
        .route("/api/projects", post(create_project))
        .route("/api/projects/{key}", get(project_by_key))
        .route("/api/projects/{key}/delete", post(delete_project))
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/search", get(search_tasks))
        .route("/api/tasks/{key}/last_commit", get(task_last_commit))
        .route("/api/tasks/{key}", get(task_by_key))
        .route("/api/tasks/{key}", post(update_task))
        .route("/api/tasks/{key}/reorder", post(reorder_task))
        .route("/api/tasks/{key}/delete", post(delete_task))
        .route("/api/tasks/{key}/archive", post(archive_task))
        .route("/api/tasks/{key}/unarchive", post(unarchive_task))
        .route("/api/tasks/{key}/logs", get(task_logs))
        .route("/api/tasks/{key}/logs", post(add_task_log))
        .route("/api/tasks/{key}/diffs", get(task_diffs))
        .route("/api/projects/{key}/logs", get(project_logs))
        .route("/api/projects/{key}/logs", post(add_project_log))
        .route("/api/tasks/{key}/attachments", get(task_attachments))
        .route("/api/tasks/{key}/attachments", post(upload_attachment))
        .route("/api/tasks/{key}/links", get(task_links))
        .route("/api/tasks/{key}/links/restore", post(restore_task_link))
        .route("/api/tasks/{key}/links/{id}/delete", post(delete_task_link))
        .route("/api/tasks/{key}/prs", get(task_prs))
        .route("/api/scopes", get(list_scopes))
        .route("/api/markdown/preview", post(markdown_preview))
        .route("/api/markdown/lines", post(markdown_lines))
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

// Create a new project via the web UI.
#[derive(Deserialize)]
struct CreateProjectRequest {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

async fn create_project(state: State<Arc<ServerState>>, Json(body): Json<CreateProjectRequest>) -> Response {
    let lun = state.lun.lock().await;
    let spec = ProjectSpec {
        name: body.name,
        status: body.status,
        message: body.message,
        user: Some("web".to_string()),
    };
    match lun.create_project(spec) {
        Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
        Err(e) => api_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn project_by_key(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    match lun.project_by_key(&key) {
        Ok(p) => {
            let n = lun.project_task_count(p.id).unwrap_or(0);
            let mut v = serde_json::to_value(&p).unwrap_or_default();
            v["task_count"] = serde_json::json!(n);
            Json(v).into_response()
        }
        Err(e) => api_err(StatusCode::NOT_FOUND, &e.to_string()),
    }
}

#[derive(Deserialize, Default)]
struct DeleteProjectRequest {
    /// Project key (P-xxxx) to move the tasks to.
    #[serde(default)]
    move_to: Option<String>,
    #[serde(default)]
    cascade: bool,
    #[serde(default)]
    message: Option<String>,
}

async fn delete_project(
    state: State<Arc<ServerState>>,
    Path(key): Path<String>,
    body: Option<Json<DeleteProjectRequest>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let lun = state.lun.lock().await;
    let project = match lun.project_by_key(&key) {
        Ok(p) => p,
        Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    if body.cascade && body.move_to.is_some() {
        return api_err(StatusCode::BAD_REQUEST, "move_to and cascade are mutually exclusive");
    }
    let mode = if let Some(target_key) = &body.move_to {
        match lun.project_by_key(target_key) {
            Ok(t) => crate::db::ProjectDeleteMode::MoveTo(t.id),
            Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
        }
    } else if body.cascade {
        crate::db::ProjectDeleteMode::Cascade
    } else {
        crate::db::ProjectDeleteMode::RefuseIfNotEmpty
    };
    match lun.delete_project(project.id, mode, body.message.as_deref(), Some("web")) {
        Ok(done) => {
            // Remove managed attachment copies (inside <board>/.lun/attachments/).
            let managed = state.data_dir.join("attachments");
            for p in &done.attachment_paths {
                let path = std::path::Path::new(p);
                let abs = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    state.data_dir.parent().map(|b| b.join(path)).unwrap_or_else(|| path.to_path_buf())
                };
                if abs.starts_with(&managed) {
                    let _ = std::fs::remove_file(&abs);
                }
            }
            Json(serde_json::json!({
                "deleted": done.project,
                "tasks_moved": done.tasks_moved,
                "tasks_deleted": done.tasks_deleted,
                "moved_to": done.moved_to,
            }))
            .into_response()
        }
        Err(e) => {
            let status = match e.kind() {
                "not-found" => StatusCode::NOT_FOUND,
                "invalid" => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            api_err(status, &e.to_string())
        }
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
        Some("updated") => TaskSort::Updated,
        Some("board") | _ => TaskSort::Board,
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

#[derive(Deserialize)]
struct ReorderBody {
    dir: i64,
}

/// Move a card one row down (dir >= 0) or up (dir < 0) within its column.
async fn reorder_task(
    state: State<Arc<ServerState>>,
    Path(key): Path<String>,
    Json(body): Json<ReorderBody>,
) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t,
        Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.reorder_task(task.id, if body.dir >= 0 { 1 } else { -1 }) {
        Ok(t) => Json(t).into_response(),
        Err(e) => api_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

/// Permanently delete a task (`lun rm task`). The web UI's Delete task
/// button; Archive stays on `/archive` (the `x` shortcut).
async fn delete_task(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t,
        Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.delete_task(task.id, None, Some("web")) {
        Ok(done) => {
            let managed = state.data_dir.join("attachments");
            for p in &done.attachment_paths {
                let path = std::path::Path::new(p);
                let abs = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    state.data_dir.parent().map(|b| b.join(path)).unwrap_or_else(|| path.to_path_buf())
                };
                if abs.starts_with(&managed) {
                    let _ = std::fs::remove_file(&abs);
                }
            }
            Json(serde_json::json!({
                "deleted": done.task,
                "log_entries": done.log_entries,
                "links": done.links,
                "attachments": done.attachments,
                "prs": done.prs,
            }))
            .into_response()
        }
        Err(e) => api_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn archive_task(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t,
        Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.archive_task(task.id, None, Some("web")) {
        Ok(t) => Json(t).into_response(),
        Err(e) => api_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn unarchive_task(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t,
        Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.unarchive_task(task.id, Some("web")) {
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

#[derive(serde::Deserialize)]
struct LogMessage { message: String }

async fn add_task_log(
    state: State<Arc<ServerState>>,
    Path(key): Path<String>,
    Json(body): Json<LogMessage>,
) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    if body.message.trim().is_empty() {
        return api_err(StatusCode::BAD_REQUEST, "message is empty");
    }
    let id = match lun.log("task", task.id, "COMMIT", &body.message, "{}", Some("web")) {
        Ok(id) => id, Err(e) => return api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
}

async fn task_diffs(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.get_note_diffs(task.id) {
        Ok(d) => Json(d).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn project_logs(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let project = match lun.project_by_key(&key) {
        Ok(p) => p, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.project_logs(project.id) {
        Ok(l) => Json(l).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn add_project_log(
    state: State<Arc<ServerState>>,
    Path(key): Path<String>,
    Json(body): Json<LogMessage>,
) -> Response {
    let lun = state.lun.lock().await;
    let project = match lun.project_by_key(&key) {
        Ok(p) => p, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    if body.message.trim().is_empty() {
        return api_err(StatusCode::BAD_REQUEST, "message is empty");
    }
    let id = match lun.log("project", project.id, "COMMIT", &body.message, "{}", Some("web")) {
        Ok(id) => id, Err(e) => return api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response()
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

async fn delete_task_link(
    state: State<Arc<ServerState>>,
    Path((key, id)): Path<(String, i64)>,
) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.task_links(task.id) {
        Ok(l) if l.iter().any(|x| x.id == id) => {}
        Ok(_) => return api_err(StatusCode::NOT_FOUND, "link not on this task"),
        Err(e) => return api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
    match lun.delete_link(id, Some("web")) {
        Ok(l) => Json(l).into_response(),
        Err(e) => api_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn restore_task_link(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    let task = match lun.task_by_key(&key) {
        Ok(t) => t, Err(e) => return api_err(StatusCode::NOT_FOUND, &e.to_string()),
    };
    match lun.restore_last_link(LinkTarget::Task(task.id), Some("web")) {
        Ok(Some(l)) => Json(l).into_response(),
        Ok(None) => api_err(StatusCode::NOT_FOUND, "no deleted link to restore"),
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

async fn task_last_commit(state: State<Arc<ServerState>>, Path(key): Path<String>) -> Response {
    let lun = state.lun.lock().await;
    // Look up task to get its id
    let task = match lun.task_by_key(&key) {
        Ok(t) => t,
        Err(_) => return api_err(StatusCode::NOT_FOUND, "task not found"),
    };
    // Return the most recent log entry for the task
    let msg = match lun.task_logs(task.id) {
        Ok(logs) => {
            if let Some(first) = logs.first() {
                Some(first.message.clone())
            } else {
                None
            }
        }
        Err(_) => None,
    };
    let json = serde_json::json!({"message": msg});
    Json(json).into_response()
}

/// Which board this server is serving (shown next to the logo).
async fn board_info(state: State<Arc<ServerState>>) -> Response {
    Json(serde_json::json!({
        "root": state.board_root,
        "display": state.board_display,
    }))
    .into_response()
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

/// Most units one `/api/markdown/lines` request may render.
const MAX_MARKDOWN_UNITS: usize = 2000;

#[derive(Deserialize)]
struct MarkdownLinesRequest { #[serde(default)] items: Vec<MarkdownUnit> }

#[derive(Deserialize)]
struct MarkdownUnit {
    /// Raw source of the unit: one line, or a whole multi-line `$$` block.
    #[serde(default)] t: String,
    /// Unit kind from the client's scan: line | code | fence | math | thead | trow | tsep.
    #[serde(default)] k: String,
}

/// Live notes editor: renders each unit on its own, so the client can show
/// rendered lines next to the one raw line being edited. Returns one inner
/// HTML fragment per item, in request order.
async fn markdown_lines(Json(body): Json<MarkdownLinesRequest>) -> Response {
    let html: Vec<String> = body
        .items
        .iter()
        .take(MAX_MARKDOWN_UNITS)
        .map(|u| render_markdown_unit(&u.t, &u.k))
        .collect();
    Json(serde_json::json!({"html": html})).into_response()
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

/// Token for a math region in the placeholder-substituted text.
/// Alphanumeric only, so it survives pulldown_cmark as an intact text node.
fn math_token(idx: usize) -> String {
    format!("LUNMATH{:04x}Q", idx)
}

/// HTML-escape for use inside a double-quoted attribute.
/// Backslashes are left untouched (KaTeX needs them verbatim).
fn attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Extract `$$display$$` and `$inline$` math regions out of the text.
/// Returns (text_with_tokens, [(tex, is_display)]).
fn extract_math(text: &str) -> (String, Vec<(String, bool)>) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut math: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    while i < n {
        if chars[i] == '$' {
            let display = i + 1 < n && chars[i + 1] == '$';
            let open_len = if display { 2 } else { 1 };
            let body_start = i + open_len;
            if body_start < n && chars[body_start] != ' ' {
                // Find the closing delimiter.
                let mut close: Option<usize> = None;
                let mut j = body_start;
                while j < n {
                    if chars[j] == '$'
                        && (!display || (j + 1 < n && chars[j + 1] == '$'))
                        && (display || j + 1 >= n || chars[j + 1] != '$')
                    {
                        close = Some(j);
                        break;
                    }
                    j += 1;
                }
                if let Some(c) = close {
                    let body: String = chars[body_start..c].iter().collect();
                    if !body.trim().is_empty() {
                        let idx = math.len();
                        math.push((body, display));
                        out.push_str(&math_token(idx));
                        i = c + open_len;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, math)
}

/// Escape text for inclusion in HTML element content.
fn text_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Scheme check for user-provided link/image URLs.
fn url_scheme_ok(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if let Some(colon) = lower.find(':') {
        if colon < 8 {
            return matches!(&lower[..colon],
                "http" | "https" | "mailto" | "obsidian" | "file"
            );
        }
    }
    // Relative / fragment URLs are fine.
    !lower.starts_with("//")
}

/// Render pulldown_cmark events to HTML.
///
/// `pulldown_cmark::html::push_html` drops `TaskListMarker` events, which
/// silently breaks `- [x]` / `- [ ]` checklist rendering. This renderer
/// emits real (disabled) checkboxes for task-list items. Output is
/// defense-in-depth sanitized by ammonia in render_markdown.
fn render_events(events: Vec<Event>) -> String {
    let mut out = String::new();
    let mut bad_link = false;
    for ev in events {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Heading { level, .. } => {
                    let n = match level {
                        pulldown_cmark::HeadingLevel::H1 => 1,
                        pulldown_cmark::HeadingLevel::H2 => 2,
                        pulldown_cmark::HeadingLevel::H3 => 3,
                        pulldown_cmark::HeadingLevel::H4 => 4,
                        pulldown_cmark::HeadingLevel::H5 => 5,
                        pulldown_cmark::HeadingLevel::H6 => 6,
                    };
                    out.push_str(&format!("<h{}>", n));
                }
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(kind) => match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => {
                        out.push_str("<pre><code");
                        let info = info.trim();
                        if !info.is_empty() {
                            out.push_str(&format!(" class=\"language-{}\"", attr_escape(info)));
                        }
                        out.push('>');
                    }
                    pulldown_cmark::CodeBlockKind::Indented => out.push_str("<pre><code>"),
                },
                Tag::Link { dest_url, .. } => {
                    if url_scheme_ok(&dest_url) {
                        out.push_str(&format!(
                            "<a href=\"{}\" rel=\"noopener noreferrer\">",
                            attr_escape(&dest_url)
                        ));
                        bad_link = false;
                    } else {
                        bad_link = true;
                    }
                }
                Tag::Image { dest_url, title, .. } => {
                    if url_scheme_ok(&dest_url) {
                        out.push_str(&format!(
                            "<img src=\"{}\" alt=\"{}\">",
                            attr_escape(&dest_url),
                            attr_escape(&title)
                        ));
                    }
                }
                Tag::Table(_) => out.push_str("<table>"),
                Tag::TableHead => out.push_str("<thead>"),
                Tag::TableRow => out.push_str("<tr>"),
                Tag::TableCell => out.push_str("<td>"),
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong>"),
                Tag::Strikethrough => out.push_str("<del>"),
                Tag::List(None) => out.push_str("<ul>"),
                Tag::List(Some(start)) => out.push_str(&format!("<ol start=\"{}\">", start)),
                Tag::Item => out.push_str("<li>"),
                // Raw HTML blocks, footnotes, definition lists: not rendered.
                _ => {}
            },
            Event::End(tag) => match tag {
                pulldown_cmark::TagEnd::Paragraph => out.push_str("</p>"),
                pulldown_cmark::TagEnd::Heading(level) => {
                    let n = match level {
                        pulldown_cmark::HeadingLevel::H1 => 1,
                        pulldown_cmark::HeadingLevel::H2 => 2,
                        pulldown_cmark::HeadingLevel::H3 => 3,
                        pulldown_cmark::HeadingLevel::H4 => 4,
                        pulldown_cmark::HeadingLevel::H5 => 5,
                        pulldown_cmark::HeadingLevel::H6 => 6,
                    };
                    out.push_str(&format!("</h{}>", n));
                }
                pulldown_cmark::TagEnd::BlockQuote(_) => out.push_str("</blockquote>"),
                pulldown_cmark::TagEnd::CodeBlock => out.push_str("</code></pre>"),
                pulldown_cmark::TagEnd::Link => {
                    if !bad_link {
                        out.push_str("</a>");
                    }
                    bad_link = false;
                }
                pulldown_cmark::TagEnd::Table => out.push_str("</table>"),
                pulldown_cmark::TagEnd::TableHead => out.push_str("</thead>"),
                pulldown_cmark::TagEnd::TableRow => out.push_str("</tr>"),
                pulldown_cmark::TagEnd::TableCell => out.push_str("</td>"),
                pulldown_cmark::TagEnd::Emphasis => out.push_str("</em>"),
                pulldown_cmark::TagEnd::Strong => out.push_str("</strong>"),
                pulldown_cmark::TagEnd::Strikethrough => out.push_str("</del>"),
                pulldown_cmark::TagEnd::List(false) => out.push_str("</ul>"),
                pulldown_cmark::TagEnd::List(true) => out.push_str("</ol>"),
                pulldown_cmark::TagEnd::Item => out.push_str("</li>"),
                _ => {}
            },
            Event::Text(s) => out.push_str(&text_escape(&s)),
            Event::Code(s) => out.push_str(&format!("<code>{}</code>", text_escape(&s))),
            Event::TaskListMarker(checked) => {
                // The <li> was just emitted; tag it and add the checkbox.
                if let Some(pos) = out.rfind("<li>") {
                    out.replace_range(pos..pos + 4, "<li class=\"task-item\">");
                }
                out.push_str(&format!(
                    "<input type=\"checkbox\" disabled{}> ",
                    if checked { " checked" } else { "" }
                ));
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("<br>"),
            Event::Rule => out.push_str("<hr>"),
            // Raw HTML passes through unmodified by pulldown: drop it.
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(_) => {}
            // Math is pre-extracted in render_markdown, but drop defensively.
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }
    out
}

pub fn render_markdown(text: &str) -> String {
    let text = resolve_wikilinks(text);
    // Extract math before markdown parsing: pulldown_cmark mangles backslashes
    // inside its math spans (emits doubled backslashes), which breaks KaTeX.
    // The raw TeX travels in a data-tex attribute; the frontend calls
    // katex.render() on each .lun-math element.
    let (no_math, math) = extract_math(&text);
    let parser = pulldown_cmark::Parser::new_ext(&no_math, MdOptions::all());
    let html = render_events(parser.collect());
    use std::collections::HashSet;
    let mut builder = ammonia::Builder::new();
    let mut schemes = HashSet::new();
    for s in ["http", "https", "ftp", "mailto", "obsidian", "file"] {
        schemes.insert(s);
    }
    builder.url_schemes(schemes);
    // Checkboxes for markdown task lists (- [ ] / - [x])
    builder.add_tags(["input"]);
    {
        let mut tag_attrs = builder.clone_tag_attributes();
        tag_attrs.entry("span").or_insert_with(HashSet::new).insert("class");
        for tags in tag_attrs.values_mut() {
            tags.insert("class");
        }
        tag_attrs.entry("li").or_insert_with(HashSet::new).insert("class");
        tag_attrs.entry("input").or_insert_with(HashSet::new).extend(["type", "checked", "disabled"]);
        builder.tag_attributes(tag_attrs);
    }
    let mut html = builder.clean(&html).to_string();
    // Replace math tokens with KaTeX spans carrying the raw TeX (single
    // backslashes, HTML-escaped for the attribute). The frontend renders them
    // via katex.render(el.dataset.tex, el, {displayMode}).
    for (idx, (tex, display)) in math.iter().enumerate() {
        let token = math_token(idx);
        let cls = if *display { "lun-math lun-math-display" } else { "lun-math" };
        let span = format!(
            "<span class=\"{}\" data-tex=\"{}\" data-display=\"{}\"></span>",
            cls,
            attr_escape(tex),
            if *display { "1" } else { "0" }
        );
        html = html.replace(&token, &span);
    }
    html
}

/// Inner HTML for one unit of the live notes editor.
///
/// The client wraps the fragment in its own block element and shows the raw
/// source in a textarea while the unit holds the caret, so the output carries
/// only what the unit needs: inline HTML, a list item, a table row or cell, an
/// `<hr>`, or a KaTeX placeholder. Inline content goes through the same
/// `render_markdown` pipeline as the full preview, so sanitizing and math
/// behave the same in both views.
pub fn render_markdown_unit(text: &str, kind: &str) -> String {
    match kind {
        // Code and fence lines are literal text; the wrapper keeps spacing.
        "code" | "fence" => unit_or_br(&text_escape(text)),
        "math" => render_math_unit(text),
        "thead" => render_table_row(text, true),
        "trow" => render_table_row(text, false),
        "tsep" => String::new(),
        _ => render_line_unit(text),
    }
}

/// A `<br>` for an empty unit, so the line keeps its height.
fn unit_or_br(inner: &str) -> String {
    if inner.trim().is_empty() {
        "<br>".to_string()
    } else {
        inner.to_string()
    }
}

/// KaTeX placeholder for a `$$ ... $$` block (the client renders it).
fn render_math_unit(text: &str) -> String {
    let t = text.trim();
    let inner = t.strip_prefix("$$").unwrap_or(t);
    let inner = inner.strip_suffix("$$").unwrap_or(inner).trim();
    if inner.is_empty() {
        return unit_or_br(&text_escape(text));
    }
    format!(
        "<span class=\"lun-math lun-math-display\" data-tex=\"{}\" data-display=\"1\"></span>",
        attr_escape(inner)
    )
}

/// One table row as flex cells (the client lays rows out as a table).
fn render_table_row(text: &str, header: bool) -> String {
    let mut out = String::from(if header { "<div class=\"md-tr md-thead\">" } else { "<div class=\"md-tr\">" });
    for cell in split_table_cells(text) {
        out.push_str("<span class=\"md-td\">");
        out.push_str(&render_inline(cell.trim()));
        out.push_str("</span>");
    }
    out.push_str("</div>");
    out
}

/// Split a table row into cells. Leading and trailing pipes are optional; `\|`
/// is a literal pipe inside a cell.
fn split_table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        cells.push(cur);
    }
    cells
}

/// Inner HTML for an ordinary line: rule, heading, quote, list item or
/// paragraph. The client decides the wrapper class from the same markers.
fn render_line_unit(text: &str) -> String {
    if is_hr_line(text) {
        return "<hr>".to_string();
    }
    if let Some((_, content)) = heading_parts(text) {
        return unit_or_br(&render_inline(content));
    }
    if let Some(content) = quote_content(text) {
        return unit_or_br(&render_inline(content));
    }
    if let Some(item) = list_item_parts(text) {
        return render_list_item(&item);
    }
    unit_or_br(&render_inline(text.trim_start()))
}

/// Render inline markdown only: the full pipeline, minus the wrapping `<p>`.
fn render_inline(content: &str) -> String {
    let html = render_markdown(content);
    let t = html.trim();
    let t = t
        .strip_prefix("<p>")
        .and_then(|s| s.strip_suffix("</p>"))
        .unwrap_or(t);
    t.trim().to_string()
}

/// `---`, `***` or `___` (three or more, spaces allowed between).
fn is_hr_line(text: &str) -> bool {
    let t = text.trim();
    let mut chars = t.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else { return false };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    rest.len() >= 2 && rest.iter().all(|c| *c == first)
}

/// `# Title` through `###### Title`: (level, content). Closing `#`s dropped.
fn heading_parts(text: &str) -> Option<(usize, &str)> {
    let t = text.trim_start_matches(' ');
    if text.len() - t.len() > 3 {
        return None;
    }
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let c = rest.trim();
    let stripped = c.trim_end_matches('#');
    let content = if stripped.len() < c.len() && (stripped.is_empty() || stripped.ends_with(' ')) {
        stripped.trim_end()
    } else {
        c
    };
    Some((hashes, content))
}

/// `> quote` (nested `>` markers are stripped too).
fn quote_content(text: &str) -> Option<&str> {
    let t = text.trim_start_matches(' ');
    if text.len() - t.len() > 3 || !t.starts_with('>') {
        return None;
    }
    let mut rest = t;
    while let Some(r) = rest.strip_prefix('>') {
        rest = r.strip_prefix(' ').unwrap_or(r);
    }
    Some(rest)
}

struct ListItem<'a> {
    /// Leading spaces (nesting depth).
    indent: usize,
    /// Ordered list number, or None for a bullet.
    ordered: Option<u64>,
    /// `[ ]` / `[x]` task checkbox state, or None for a plain item.
    task: Option<bool>,
    content: &'a str,
}

/// `- item`, `* item`, `+ item`, `1. item`, `1) item`, `- [ ] task`.
fn list_item_parts(text: &str) -> Option<ListItem<'_>> {
    let indent = text.len() - text.trim_start_matches(' ').len();
    let t = &text[indent..];
    let (ordered, rest) = match t.chars().next() {
        Some('-') | Some('*') | Some('+') => {
            if t.len() > 1 && !t[1..].starts_with(' ') {
                return None;
            }
            (None, &t[1..])
        }
        _ => {
            let digits = t.bytes().take_while(|b| b.is_ascii_digit()).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            let delim = t.as_bytes().get(digits).copied()?;
            if delim != b'.' && delim != b')' {
                return None;
            }
            let after = &t[digits + 1..];
            if !after.is_empty() && !after.starts_with(' ') {
                return None;
            }
            (Some(t[..digits].parse::<u64>().ok()?), after)
        }
    };
    let rest = rest.trim_start_matches(' ');
    let mut task = None;
    let mut content = rest;
    for (pat, checked) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
        if let Some(r) = rest.strip_prefix(pat) {
            if r.is_empty() || r.starts_with(' ') {
                task = Some(checked);
                content = r.trim_start_matches(' ');
                break;
            }
        }
    }
    Some(ListItem { indent, ordered, task, content })
}

fn render_list_item(item: &ListItem) -> String {
    let mut class = String::from("md-li");
    if item.task.is_some() {
        class.push_str(" md-task");
    }
    let mut attrs = String::new();
    if let Some(n) = item.ordered {
        attrs.push_str(&format!(" data-n=\"{}\"", n));
    }
    if item.indent > 0 {
        attrs.push_str(&format!(" style=\"margin-left:{:.2}em\"", item.indent as f32 * 0.28));
    }
    let mut out = format!("<div class=\"{}\"{}>", class, attrs);
    match item.task {
        Some(true) => out.push_str("<input type=\"checkbox\" disabled checked>"),
        Some(false) => out.push_str("<input type=\"checkbox\" disabled>"),
        None => {}
    }
    let inline = render_inline(item.content);
    if inline.is_empty() {
        out.push_str("<br>");
    } else {
        out.push_str(&inline);
    }
    out.push_str("</div>");
    out
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
    fn test_render_markdown_inline_math_token() {
        let html = render_markdown("some $x^2$ math");
        assert!(html.contains("lun-math"), "inline math span expected: {}", html);
        assert!(html.contains("data-tex=\"x^2\""), "raw TeX in data-tex: {}", html);
        assert!(!html.contains("LUNMATH"), "no leftover tokens: {}", html);
    }

    #[test]
    fn test_render_markdown_backslashes_not_doubled() {
        let html = render_markdown("inline $a\\propto b$ and display $$10^{-4}\\langle f\\rangle$$");
        assert!(html.contains(r#"data-tex="a\propto b""#), "single backslash kept: {}", html);
        assert!(html.contains(r##"data-tex="10^{-4}\langle f\rangle"##), "display math backslashes kept: {}", html);
        assert!(!html.contains("\\\\"), "no doubled backslashes: {}", html);
        assert!(html.contains("data-display=\"1\""), "display flag set: {}", html);
    }

    #[test]
    fn test_render_markdown_tasklist_checkboxes() {
        let html = render_markdown("- [x] done item\n- [ ] open item");
        assert!(html.contains("task-item"), "task-item class present: {}", html);
        assert!(html.contains(r#"<input type="checkbox" disabled="" checked="""#), "checked box: {}", html);
        assert_eq!(html.matches("checkbox").count(), 2, "two checkboxes: {}", html);
    }

    #[test]
    fn test_render_markdown_headings_and_links() {
        let html = render_markdown("# Title\n\n[ok](https://example.com) [bad](javascript:alert(1))");
        assert!(html.contains("<h1>Title</h1>"), "h1: {}", html);
        assert!(html.contains("href=\"https://example.com\""), "safe link: {}", html);
        assert!(!html.contains("javascript:"), "bad scheme stripped: {}", html);
    }

    #[test]
    fn test_render_markdown_wikilinks() {
        let html = render_markdown("[[my note]]");
        assert!(html.contains("obsidian://open?vault=personal"),
            "wikilink should resolve to obsidian:// URL: {}", html);
    }

    #[test]
    fn test_render_markdown_block_math() {
        let html = render_markdown("$$x^2 + y^2$$");
        assert!(html.contains("lun-math-display"), "display math class: {}", html);
        assert!(html.contains("data-tex=\"x^2 + y^2\""), "display TeX verbatim: {}", html);
    }

    #[test]
    fn test_render_markdown_no_math_passthrough() {
        let html = render_markdown("no math here, $5 and $10 prices");
        // "$5 and $10" looks like math to the extractor; KaTeX will just
        // fail softly on it client-side. Either way, no leftover tokens.
        assert!(!html.contains("LUNMATH"), "no leftover tokens: {}", html);
    }

    // ---- Live notes editor: per-unit rendering -----------------------

    #[test]
    fn unit_heading_drops_hashes_and_keeps_inline_markup() {
        let html = render_markdown_unit("## Plan **now**", "line");
        assert!(html.contains("Plan <strong>now</strong>"), "{}", html);
        assert!(!html.contains("##"), "hashes must be dropped: {}", html);
        assert!(!html.contains("<h2>") && !html.contains("<p>"), "inner only: {}", html);
    }

    #[test]
    fn unit_hash_without_space_is_a_paragraph() {
        let html = render_markdown_unit("#hashtag", "line");
        assert!(html.contains("#hashtag"), "{}", html);
        assert!(!html.contains("<h1>"), "{}", html);
    }

    #[test]
    fn unit_rules_and_near_misses() {
        assert_eq!(render_markdown_unit("---", "line"), "<hr>");
        assert_eq!(render_markdown_unit("- - -", "line"), "<hr>");
        assert!(!render_markdown_unit("--", "line").contains("<hr>"));
    }

    #[test]
    fn unit_quote_and_bullet_wrappers() {
        let q = render_markdown_unit("> quoted _it_", "line");
        assert!(q.contains("quoted <em>it</em>"), "{}", q);
        assert!(!q.contains("&gt;"), "marker must be stripped: {}", q);

        let b = render_markdown_unit("- item", "line");
        assert!(b.starts_with("<div class=\"md-li\">") && b.contains("item"), "{}", b);
        assert!(!b.contains("<ul>"), "{}", b);
    }

    #[test]
    fn unit_ordered_and_nested_items() {
        let o = render_markdown_unit("3. third", "line");
        assert!(o.contains("data-n=\"3\""), "{}", o);
        assert_eq!(list_item_parts("2) x").unwrap().ordered, Some(2));
        let n = render_markdown_unit("    - deep", "line");
        assert!(n.contains("margin-left:1.12em"), "{}", n);
    }

    #[test]
    fn unit_task_items_show_checkbox_state() {
        let done = render_markdown_unit("- [x] done", "line");
        assert!(done.contains("md-task") && done.contains("checked"), "{}", done);
        assert!(done.contains("done") && !done.contains("[x]"), "{}", done);
        let open = render_markdown_unit("* [ ] open", "line");
        assert!(open.contains("md-task") && !open.contains("checked"), "{}", open);
    }

    #[test]
    fn unit_indented_text_is_not_a_code_block() {
        let html = render_markdown_unit("    not code", "line");
        assert!(!html.contains("<pre>"), "{}", html);
        assert!(html.contains("not code"), "{}", html);
    }

    #[test]
    fn unit_empty_lines_keep_height() {
        assert_eq!(render_markdown_unit("", "line"), "<br>");
        assert_eq!(render_markdown_unit("   ", "code"), "<br>");
        assert!(render_markdown_unit("- ", "line").contains("<br>"));
    }

    #[test]
    fn unit_code_and_fence_are_literal_and_escaped() {
        assert_eq!(render_markdown_unit("if a < b && c", "code"), "if a &lt; b &amp;&amp; c");
        assert_eq!(render_markdown_unit("```rust", "fence"), "```rust");
        // Markdown inside a code line is not rendered.
        assert!(render_markdown_unit("**x**", "code").contains("**x**"));
    }

    #[test]
    fn unit_math_block_keeps_raw_tex() {
        let m = render_markdown_unit("$$\n\\frac{a}{b}\n$$", "math");
        assert!(m.contains("lun-math-display"), "{}", m);
        assert!(m.contains("data-tex=\"\\frac{a}{b}\""), "single backslashes kept: {}", m);
        let empty = render_markdown_unit("$$\n$$", "math");
        assert!(!empty.contains("lun-math"), "{}", empty);
    }

    #[test]
    fn unit_inline_math_in_a_line() {
        let html = render_markdown_unit("energy $E=mc^2$ here", "line");
        assert!(html.contains("data-tex=\"E=mc^2\""), "{}", html);
        assert!(!html.contains("LUNMATH"), "{}", html);
    }

    #[test]
    fn unit_table_rows_and_cells() {
        let head = render_markdown_unit("| a | b |", "thead");
        assert!(head.contains("md-thead"), "{}", head);
        assert_eq!(head.matches("class=\"md-td\"").count(), 2, "{}", head);
        let row = render_markdown_unit("c | **d**", "trow");
        assert!(row.contains("<strong>d</strong>"), "{}", row);
        assert_eq!(row.matches("class=\"md-td\"").count(), 2, "{}", row);
        assert_eq!(render_markdown_unit("|---|---|", "tsep"), "");
    }

    #[test]
    fn unit_escaped_pipe_stays_in_its_cell() {
        let row = render_markdown_unit("| a \\| b | c |", "trow");
        assert_eq!(row.matches("class=\"md-td\"").count(), 2, "{}", row);
        assert!(row.contains("a | b"), "{}", row);
    }

    #[test]
    fn unit_sanitizes_html_and_unsafe_links() {
        let raw = render_markdown_unit("<img src=x onerror=alert(1)> hi", "line");
        assert!(!raw.contains("onerror"), "{}", raw);
        let link = render_markdown_unit("[bad](javascript:alert(1))", "line");
        assert!(!link.contains("href=\"javascript"), "{}", link);
    }

    #[test]
    fn unit_unknown_kind_renders_as_a_line() {
        assert_eq!(render_markdown_unit("plain", "nonsense"), render_markdown_unit("plain", "line"));
    }
}
