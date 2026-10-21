use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use bittopos_core::{Database, DatabaseError};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

const SESSION_COOKIE: &str = "bittopos_session";
const SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
struct AppState {
    username: Arc<str>,
    password: Arc<str>,
    db_path: Arc<PathBuf>,
    secure_cookie: bool,
    sessions: Arc<Mutex<HashMap<String, Instant>>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProjectRequest {
    name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveWorkspaceRequest {
    workspace: Value,
    expected_revision: i64,
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn db_error(db_error: DatabaseError) -> Response {
    match db_error {
        DatabaseError::NotFound(message) => error(StatusCode::NOT_FOUND, message),
        DatabaseError::RevisionConflict { current } => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "La estructura cambió.", "revision": current })),
        )
            .into_response(),
        DatabaseError::Invalid(message) => error(StatusCode::BAD_REQUEST, message),
        other => {
            eprintln!("database error: {other}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error interno de persistencia",
            )
        }
    }
}

fn open_database(state: &AppState) -> Result<Database, Response> {
    Database::open(state.db_path.as_path()).map_err(db_error)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "bittopos-server", "api_version": 1 }))
}

async fn login(State(state): State<AppState>, Json(input): Json<LoginRequest>) -> Response {
    if input.username != state.username.as_ref() || input.password != state.password.as_ref() {
        return error(StatusCode::UNAUTHORIZED, "Credenciales inválidas");
    }

    let token = Uuid::now_v7().to_string();
    let mut sessions = match state.sessions.lock() {
        Ok(sessions) => sessions,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Sesiones no disponibles"),
    };
    sessions.retain(|_, expiry| *expiry > Instant::now());
    sessions.insert(token.clone(), Instant::now() + SESSION_TTL);
    drop(sessions);

    let (same_site, secure) = if state.secure_cookie {
        ("None", "; Secure")
    } else {
        ("Lax", "")
    };
    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite={same_site}; Max-Age={}{}",
        SESSION_TTL.as_secs(),
        secure
    );
    let mut response = Json(json!({ "authenticated": true })).into_response();
    match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            response.headers_mut().insert(header::SET_COOKIE, value);
            response
        }
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "No se pudo crear la sesión",
        ),
    }
}

async fn logout(State(state): State<AppState>, request: Request) -> Response {
    if let Some(token) = cookie_value(request.headers(), SESSION_COOKIE) {
        if let Ok(mut sessions) = state.sessions.lock() {
            sessions.remove(&token);
        }
    }
    let cookie = if state.secure_cookie {
        format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=None; Secure; Max-Age=0")
    } else {
        format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
    };
    let mut response = Json(json!({ "authenticated": false })).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn me() -> Json<Value> {
    Json(json!({ "authenticated": true }))
}

async fn list_profiles(State(state): State<AppState>) -> Response {
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.list_profiles() {
        Ok(items) => Json(json!({ "items": items })).into_response(),
        Err(error) => db_error(error),
    }
}

async fn create_profile(State(state): State<AppState>) -> Response {
    let profile = json!({ "id": Uuid::now_v7().to_string() });
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.create_profile(&profile) {
        Ok(()) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(error) => db_error(error),
    }
}

async fn list_projects(State(state): State<AppState>, Path(profile_id): Path<String>) -> Response {
    if Uuid::parse_str(&profile_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de perfil inválido");
    }
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.list_projects(&profile_id) {
        Ok(items) => Json(json!({ "items": items })).into_response(),
        Err(error) => db_error(error),
    }
}

async fn create_project(
    State(state): State<AppState>,
    Path(profile_id): Path<String>,
    Json(input): Json<CreateProjectRequest>,
) -> Response {
    if Uuid::parse_str(&profile_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de perfil inválido");
    }
    if input.name.trim().is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "El nombre del proyecto no puede estar vacío",
        );
    }
    let project = json!({
        "id": Uuid::now_v7().to_string(),
        "profile_id": profile_id,
        "name": input.name.trim(),
        "description": input.description.as_deref().map(str::trim).filter(|s| !s.is_empty())
    });
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.create_project(&project) {
        Ok(()) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(error) => db_error(error),
    }
}

async fn list_resources(State(state): State<AppState>, Path(project_id): Path<String>) -> Response {
    if Uuid::parse_str(&project_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de proyecto inválido");
    }
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.list_resources(&project_id) {
        Ok(items) => Json(json!({ "items": items })).into_response(),
        Err(error) => db_error(error),
    }
}

async fn create_resource(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(resource): Json<Value>,
) -> Response {
    if Uuid::parse_str(&project_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de proyecto inválido");
    }
    let Some(object) = resource.as_object() else {
        return error(
            StatusCode::BAD_REQUEST,
            "El recurso debe ser un objeto JSON",
        );
    };
    if object.get("id").and_then(Value::as_str).is_none() {
        return error(StatusCode::BAD_REQUEST, "El recurso debe incluir un id");
    }
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.save_resource(&project_id, &resource, None) {
        Ok(()) => (StatusCode::CREATED, Json(resource)).into_response(),
        Err(error) => db_error(error),
    }
}

async fn update_resource(
    State(state): State<AppState>,
    Path((project_id, resource_id)): Path<(String, String)>,
    Json(resource): Json<Value>,
) -> Response {
    if Uuid::parse_str(&project_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de proyecto inválido");
    }
    if resource.get("id").and_then(Value::as_str) != Some(resource_id.as_str()) {
        return error(
            StatusCode::BAD_REQUEST,
            "El id del recurso debe coincidir con la URL",
        );
    }
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.save_resource(&project_id, &resource, None) {
        Ok(()) => Json(resource).into_response(),
        Err(error) => db_error(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportTomlRequest {
    content: String,
}

async fn import_resource_toml(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(input): Json<ImportTomlRequest>,
) -> Response {
    if Uuid::parse_str(&project_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de proyecto inválido");
    }
    let resource: bittopos_core::Resource = match toml::from_str(&input.content) {
        Ok(resource) => resource,
        Err(parse_error) => {
            return error(
                StatusCode::BAD_REQUEST,
                format!("TOML inválido: {parse_error}"),
            )
        }
    };
    let value = match serde_json::to_value(resource) {
        Ok(value) => value,
        Err(serialize_error) => {
            return error(
                StatusCode::BAD_REQUEST,
                format!("Recurso inválido: {serialize_error}"),
            )
        }
    };
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.save_resource(&project_id, &value, Some(&input.content)) {
        Ok(()) => (StatusCode::CREATED, Json(json!({ "resource": value }))).into_response(),
        Err(error) => db_error(error),
    }
}

async fn get_workspace(State(state): State<AppState>, Path(project_id): Path<String>) -> Response {
    if Uuid::parse_str(&project_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de proyecto inválido");
    }
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.load_workspace(&project_id) {
        Ok((workspace, revision)) => {
            Json(json!({ "workspace": workspace, "revision": revision })).into_response()
        }
        Err(error) => db_error(error),
    }
}

async fn save_workspace(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(input): Json<SaveWorkspaceRequest>,
) -> Response {
    if Uuid::parse_str(&project_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "ID de proyecto inválido");
    }
    if input.expected_revision < 0 {
        return error(
            StatusCode::BAD_REQUEST,
            "expected_revision debe ser no negativo",
        );
    }
    let mut db = match open_database(&state) {
        Ok(db) => db,
        Err(response) => return response,
    };
    match db.save_workspace(&project_id, &input.workspace, input.expected_revision) {
        Ok(revision) => {
            Json(json!({ "workspace": input.workspace, "revision": revision })).into_response()
        }
        Err(error) => db_error(error),
    }
}

async fn require_session(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(token) = cookie_value(request.headers(), SESSION_COOKIE) else {
        return error(StatusCode::UNAUTHORIZED, "Autenticación requerida");
    };
    let valid = match state.sessions.lock() {
        Ok(mut sessions) => {
            sessions.retain(|_, expiry| *expiry > Instant::now());
            sessions
                .get(&token)
                .is_some_and(|expiry| *expiry > Instant::now())
        }
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Sesiones no disponibles"),
    };
    if !valid {
        return error(StatusCode::UNAUTHORIZED, "Sesión inválida o vencida");
    }
    next.run(request).await
}

fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let password = env::var("BITTOPOS_PASSWORD")
        .map_err(|_| "Falta BITTOPOS_PASSWORD; el servidor no arrancará sin autenticación")?;
    if password.len() < 12 {
        return Err("BITTOPOS_PASSWORD debe tener al menos 12 caracteres".into());
    }

    let db_path = PathBuf::from(
        env::var("BITTOPOS_DB_PATH").unwrap_or_else(|_| "./data/bittopos.sqlite3".to_owned()),
    );
    let state = AppState {
        username: Arc::from(env::var("BITTOPOS_USERNAME").unwrap_or_else(|_| "admin".to_owned())),
        password: Arc::from(password),
        db_path: Arc::new(db_path),
        secure_cookie: env::var("SESSION_COOKIE_SECURE")
            .map(|value| value != "false")
            .unwrap_or(true),
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    let origin = env::var("WEB_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:5173".to_owned())
        .parse::<HeaderValue>()?;
    let cors = CorsLayer::new()
        .allow_origin(origin)
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE]);

    let public = Router::new()
        .route("/health", get(health))
        .route("/api/v1/login", post(login))
        .with_state(state.clone());

    let protected = Router::new()
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/me", get(me))
        .route("/api/v1/profiles", get(list_profiles).post(create_profile))
        .route(
            "/api/v1/profiles/{profile_id}/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/v1/projects/{project_id}/resources",
            get(list_resources).post(create_resource),
        )
        .route(
            "/api/v1/projects/{project_id}/resources/import-toml",
            post(import_resource_toml),
        )
        .route(
            "/api/v1/projects/{project_id}/resources/{resource_id}",
            put(update_resource),
        )
        .route(
            "/api/v1/projects/{project_id}/workspace",
            get(get_workspace).put(save_workspace),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ))
        .with_state(state);

    let app = public.merge(protected).layer(cors);
    let address: SocketAddr = env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_owned())
        .parse()?;
    let listener = TcpListener::bind(address).await?;
    println!("Bittopos server listening on {address}");
    axum::serve(listener, app).await?;
    Ok(())
}
