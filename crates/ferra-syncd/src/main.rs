//! Zero-knowledge sync node.
//!
//! The process stores opaque ciphertext only. It can verify a bearer token
//! (SHA-256 compared in constant time) and last-write-wins revisions. It has
//! no AES key and cannot read session passwords.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone, Default)]
struct App {
    accounts: Arc<Mutex<HashMap<String, Account>>>,
}

struct Account {
    token_hash: [u8; 32],
    revision: u64,
    blob: Vec<u8>,
}

#[derive(Deserialize)]
struct Register {
    account_id: String,
    token: String,
}

#[derive(Deserialize)]
struct Push {
    account_id: String,
    token: String,
    revision: u64,
    ciphertext: Vec<u8>,
}

#[derive(Deserialize)]
struct Pull {
    account_id: String,
    token: String,
}

#[derive(Serialize)]
struct BlobView {
    revision: u64,
    ciphertext: Vec<u8>,
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn token_ok(stored: &[u8; 32], token: &str) -> bool {
    use std::cmp::Ordering;
    let got = hash_token(token);
    stored.iter().zip(got.iter()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
        && stored.len().cmp(&got.len()) == Ordering::Equal
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let bind = std::env::var("FERRASSH_SYNCD_BIND").unwrap_or_else(|_| "127.0.0.1:7749".into());
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/register", post(register))
        .route("/v1/push", post(push))
        .route("/v1/pull", post(pull))
        .route("/v1/revision/{id}", get(revision))
        .with_state(App::default())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    tracing::info!("ferra-syncd listening on {bind}");
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

async fn register(State(app): State<App>, Json(body): Json<Register>) -> Result<StatusCode, StatusCode> {
    let mut map = app.accounts.lock().await;
    map.entry(body.account_id).or_insert_with(|| Account {
        token_hash: hash_token(&body.token),
        revision: 0,
        blob: Vec::new(),
    });
    Ok(StatusCode::NO_CONTENT)
}

async fn push(State(app): State<App>, Json(body): Json<Push>) -> Result<Json<BlobView>, StatusCode> {
    let mut map = app.accounts.lock().await;
    let acc = map.get_mut(&body.account_id).ok_or(StatusCode::NOT_FOUND)?;
    if !token_ok(&acc.token_hash, &body.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if body.revision < acc.revision {
        return Err(StatusCode::CONFLICT);
    }
    acc.revision = body.revision;
    acc.blob = body.ciphertext;
    Ok(Json(BlobView { revision: acc.revision, ciphertext: acc.blob.clone() }))
}

async fn pull(State(app): State<App>, Json(body): Json<Pull>) -> Result<Json<BlobView>, StatusCode> {
    let map = app.accounts.lock().await;
    let acc = map.get(&body.account_id).ok_or(StatusCode::NOT_FOUND)?;
    if !token_ok(&acc.token_hash, &body.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(BlobView { revision: acc.revision, ciphertext: acc.blob.clone() }))
}

async fn revision(State(app): State<App>, Path(id): Path<String>) -> Result<Json<u64>, StatusCode> {
    let map = app.accounts.lock().await;
    let acc = map.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(acc.revision))
}
