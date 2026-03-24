use axum::{
    Router,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
};

pub fn router() -> Router {
    Router::new()
        .route("/api/config", get(get_config))
        .route("/api/config", post(post_config))
        .route("/api/profile", get(get_profile))
        .route("/api/profile", put(set_profile))
}

async fn get_config() -> String {
    super::get_text()
}

async fn post_config(body: String) -> impl IntoResponse {
    match super::apply_and_save(&body).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn get_profile() -> &'static str {
    // TODO: Get real data from config state
    r#"{"active":"acceleration","available":["acceleration","skidpad","autocross","endurance"]}"#
}

async fn set_profile(body: String) -> impl IntoResponse {
    // TODO: Validate profile exists and set active profile in state
    tracing::info!("Profile selection: {}", body);
    StatusCode::OK
}
