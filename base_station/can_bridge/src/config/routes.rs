use axum::{
    Router,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};

pub fn router() -> Router {
    Router::new()
        .route("/config", get(get_config))
        .route("/config", post(post_config))
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
