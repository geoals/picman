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
    use axum::routing::get;

    Router::new()
        .route("/api/health", get(handlers::health))
        .route("/api/directories", get(handlers::get_directories))
        .route(
            "/api/directories/{id}/files",
            get(handlers::get_directory_files),
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
}
