mod handlers;
mod models;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::Router;
use rust_embed::Embed;
use tokio::net::TcpListener;

use crate::db::Database;

#[derive(Embed)]
#[folder = "src/serve/assets/"]
struct Assets;

pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub library_path: PathBuf,
}

pub fn run_serve(library_path: &std::path::Path, port: u16) -> Result<()> {
    let db_path = library_path.join(".picman.db");
    let db = Database::open(&db_path)?;

    let library_path = library_path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Failed to resolve library path: {}", e))?;

    let state = Arc::new(AppState {
        db: Arc::new(Mutex::new(db)),
        library_path,
    });

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let app = build_router(state);

        let addr = format!("0.0.0.0:{}", port);
        println!("Serving library on http://localhost:{}", port);
        println!("  Also available on http://0.0.0.0:{}", port);
        println!("Press Ctrl+C to stop.");

        let listener = TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    use axum::routing::{delete, get, post, put};

    Router::new()
        .route("/api/health", get(handlers::health))
        .route("/api/directories", get(handlers::get_directories))
        .route(
            "/api/directories/{id}/files",
            get(handlers::get_directory_files),
        )
        .route(
            "/api/directories/{id}/rating",
            put(handlers::set_directory_rating),
        )
        .route(
            "/api/directories/{id}/tags",
            post(handlers::add_directory_tag),
        )
        .route(
            "/api/directories/{id}/tags/{tag_name}",
            delete(handlers::remove_directory_tag),
        )
        .route("/api/tags", get(handlers::get_tags))
        .route("/api/files", get(handlers::get_filtered_files))
        .route("/thumb/{file_id}", get(handlers::serve_web_thumbnail))
        .route("/preview/{file_id}", get(handlers::serve_preview))
        .route("/dir-preview/{dir_id}", get(handlers::serve_dir_preview))
        .route("/original/{*path}", get(handlers::serve_original))
        .fallback(get(handlers::serve_embedded_asset))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        let db = Database::open_in_memory().unwrap();
        Arc::new(AppState {
            db: Arc::new(Mutex::new(db)),
            library_path: PathBuf::from("/tmp/test-library"),
        })
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_directories_endpoint_empty_db() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/directories")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_tags_endpoint_empty_db() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/tags")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_directory_files_not_found() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/directories/999/files")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 200 with empty list (directory doesn't exist but query still works)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_embedded_asset_fallback() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 200 if index.html exists, 404 otherwise
        let status = response.status();
        assert!(status == StatusCode::OK || status == StatusCode::NOT_FOUND);
    }

    fn test_state_with_dir() -> (Arc<AppState>, i64) {
        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos/vacation", None, None).unwrap();
        let state = Arc::new(AppState {
            db: Arc::new(Mutex::new(db)),
            library_path: PathBuf::from("/tmp/test-library"),
        });
        (state, dir_id)
    }

    async fn body_json(response: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_set_directory_rating() {
        let (state, dir_id) = test_state_with_dir();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/directories/{}/rating", dir_id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"rating": 3}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["rating"], 3);
    }

    #[tokio::test]
    async fn test_clear_directory_rating() {
        let (state, dir_id) = test_state_with_dir();
        // First set a rating
        {
            let db = state.db.lock().unwrap();
            db.set_directory_rating(dir_id, Some(4)).unwrap();
        }
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/directories/{}/rating", dir_id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"rating": null}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert!(json["rating"].is_null());
    }

    #[tokio::test]
    async fn test_set_directory_rating_out_of_range() {
        let (state, dir_id) = test_state_with_dir();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/directories/{}/rating", dir_id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"rating": 6}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_add_directory_tag() {
        let (state, dir_id) = test_state_with_dir();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/directories/{}/tags", dir_id))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"tag": "Travel"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let tags = json["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0], "travel"); // should be lowercased
    }

    #[tokio::test]
    async fn test_remove_directory_tag() {
        let (state, dir_id) = test_state_with_dir();
        {
            let db = state.db.lock().unwrap();
            db.add_directory_tag(dir_id, "travel").unwrap();
            db.add_directory_tag(dir_id, "2024").unwrap();
        }
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/directories/{}/tags/travel", dir_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let tags = json["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0], "2024");
    }
}
