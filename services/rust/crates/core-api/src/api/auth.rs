//! `/api/v1/auth/*` and `/api/v1/me`.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use platform::auth::AuthUser;
use platform::error::AppResult;
use serde::Deserialize;
use serde_json::Value;

use crate::api::ApiJson;
use crate::application::auth::{self, LoginIn, TokenPair};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/refresh", post(refresh))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/me", get(me))
}

fn user_agent(h: &HeaderMap) -> Option<String> {
    h.get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

async fn login(
    State(st): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<LoginIn>,
) -> AppResult<Json<TokenPair>> {
    Ok(Json(auth::login(&st, &body, user_agent(&headers)).await?))
}

#[derive(Debug, Deserialize)]
struct RefreshIn {
    refresh_token: String,
}

async fn refresh(
    State(st): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RefreshIn>,
) -> AppResult<Json<TokenPair>> {
    Ok(Json(
        auth::refresh(&st, &body.refresh_token, user_agent(&headers)).await?,
    ))
}

#[derive(Debug, Default, Deserialize)]
struct LogoutIn {
    refresh_token: Option<String>,
}

async fn logout(
    State(st): State<AppState>,
    user: AuthUser,
    body: axum::body::Bytes,
) -> AppResult<StatusCode> {
    // The body is optional: no body = revoke all of the user's refresh tokens.
    let token = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<LogoutIn>(&body)
            .map_err(|e| platform::error::AppError::BadRequest(format!("invalid JSON body: {e}")))?
            .refresh_token
    };
    auth::logout(&st, user.claims.sub, token.as_deref()).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(st): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    Ok(Json(auth::me(&st, user.claims.sub).await?))
}
