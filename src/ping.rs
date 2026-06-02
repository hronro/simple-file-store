use axum::response::Json;
use serde_json::{Value, json};

use crate::auth::Claims;

pub const ROUTE_PATH: &str = "/ping";

pub async fn get(claims: Option<Claims>) -> Json<Value> {
    Json(json!({ "pong": claims.map(|c| c.sub) }))
}
