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
            "/api/duplicates/summary",
            get(handlers::get_duplicates_summary),
        )
        .route("/api/duplicates", get(handlers::get_duplicates))
        .route("/api/duplicates/trash", post(handlers::trash_files))
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

    // ==================== Duplicates Tests ====================

    fn test_state_with_duplicates() -> Arc<AppState> {
        let db = Database::open_in_memory().unwrap();
        let dir1 = db.insert_directory("photos/vacation", None, None).unwrap();
        let dir2 = db.insert_directory("backup/2024", None, None).unwrap();

        // Exact duplicates: same hash in two folders
        let f1 = db
            .insert_file_with_dimensions(dir1, "beach.jpg", 4200000, 100, Some("image"), Some(4032), Some(3024))
            .unwrap();
        let f2 = db
            .insert_file_with_dimensions(dir2, "beach_copy.jpg", 4200000, 101, Some("image"), Some(4032), Some(3024))
            .unwrap();
        db.set_file_hash(f1, "aabbccdd").unwrap();
        db.set_file_hash(f2, "aabbccdd").unwrap();

        // Another pair of exact duplicates
        let f3 = db
            .insert_file_with_dimensions(dir1, "sunset.jpg", 3000000, 200, Some("image"), Some(1920), Some(1080))
            .unwrap();
        let f4 = db
            .insert_file_with_dimensions(dir2, "sunset_copy.jpg", 3000000, 201, Some("image"), Some(1920), Some(1080))
            .unwrap();
        db.set_file_hash(f3, "eeff0011").unwrap();
        db.set_file_hash(f4, "eeff0011").unwrap();

        // Unique file (not a duplicate)
        let f5 = db
            .insert_file(dir1, "unique.jpg", 1000, 300, Some("image"))
            .unwrap();
        db.set_file_hash(f5, "unique123").unwrap();

        Arc::new(AppState {
            db: Arc::new(Mutex::new(db)),
            library_path: PathBuf::from("/tmp/test-library"),
        })
    }

    #[tokio::test]
    async fn test_duplicates_summary() {
        let state = test_state_with_duplicates();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/duplicates/summary?threshold=8")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["exact_groups"], 2);
        assert_eq!(json["exact_files"], 4);
    }

    #[tokio::test]
    async fn test_duplicates_summary_empty_db() {
        let state = test_state();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/duplicates/summary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["exact_groups"], 0);
        assert_eq!(json["exact_files"], 0);
        assert_eq!(json["similar_groups"], 0);
        assert_eq!(json["similar_files"], 0);
    }

    #[tokio::test]
    async fn test_get_duplicates_exact() {
        let state = test_state_with_duplicates();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/duplicates?type=exact")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["total_groups"], 2);
        assert_eq!(json["page"], 1);

        let groups = json["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);

        // Each group should have 2 files
        for group in groups {
            assert_eq!(group["match_type"], "exact");
            assert!(group["hash"].is_string());
            let files = group["files"].as_array().unwrap();
            assert_eq!(files.len(), 2);
            // Auto-suggest should pick a file
            assert!(group["suggested_keep_id"].as_i64().unwrap() > 0);
        }
    }

    #[tokio::test]
    async fn test_get_duplicates_auto_suggest_highest_resolution() {
        let state = {
            let db = Database::open_in_memory().unwrap();
            let dir1 = db.insert_directory("photos", None, None).unwrap();
            let dir2 = db.insert_directory("backup", None, None).unwrap();

            // Higher resolution file
            let f1 = db
                .insert_file_with_dimensions(dir1, "high_res.jpg", 5000000, 100, Some("image"), Some(4032), Some(3024))
                .unwrap();
            // Lower resolution file
            let f2 = db
                .insert_file_with_dimensions(dir2, "low_res.jpg", 2000000, 101, Some("image"), Some(1920), Some(1080))
                .unwrap();
            db.set_file_hash(f1, "samehash").unwrap();
            db.set_file_hash(f2, "samehash").unwrap();

            Arc::new(AppState {
                db: Arc::new(Mutex::new(db)),
                library_path: PathBuf::from("/tmp/test-library"),
            })
        };

        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/duplicates?type=exact")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = body_json(response).await;
        let group = &json["groups"][0];
        let suggested_id = group["suggested_keep_id"].as_i64().unwrap();

        // The file with 4032×3024 should be suggested (higher resolution)
        let high_res_file = group["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["width"].as_i64() == Some(4032))
            .unwrap();
        assert_eq!(suggested_id, high_res_file["id"].as_i64().unwrap());
    }

    #[tokio::test]
    async fn test_get_duplicates_folder_super_groups() {
        // Two groups share the same pair of folders → should create a super-group
        let state = test_state_with_duplicates();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/duplicates?type=exact")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = body_json(response).await;
        let super_groups = json["folder_super_groups"].as_array().unwrap();

        // Both groups have photos/vacation + backup/2024 → one super-group
        assert_eq!(super_groups.len(), 1);
        let sg = &super_groups[0];
        let folders = sg["folders"].as_array().unwrap();
        assert_eq!(folders.len(), 2);
        let group_indices = sg["group_indices"].as_array().unwrap();
        assert_eq!(group_indices.len(), 2);
    }

    #[tokio::test]
    async fn test_get_duplicates_pagination() {
        let state = test_state_with_duplicates();
        let app = build_router(state);

        // Request page 1 with per_page=1
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/duplicates?type=exact&page=1&per_page=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = body_json(response).await;
        assert_eq!(json["total_groups"], 2);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 1);
        assert_eq!(json["groups"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_trash_files_empty() {
        let state = test_state();
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/duplicates/trash")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"file_ids": []}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["trashed"], 0);
        assert_eq!(json["errors"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_trash_files_moves_to_trash_and_removes_from_db() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let library_path = tmp_dir.path().to_path_buf();

        // Create directory structure and files
        let photos_dir = library_path.join("photos");
        std::fs::create_dir_all(&photos_dir).unwrap();
        std::fs::write(photos_dir.join("dupe.jpg"), b"fake image data").unwrap();

        let db = Database::open_in_memory().unwrap();
        let dir_id = db.insert_directory("photos", None, None).unwrap();
        let file_id = db
            .insert_file(dir_id, "dupe.jpg", 15, 100, Some("image"))
            .unwrap();

        let state = Arc::new(AppState {
            db: Arc::new(Mutex::new(db)),
            library_path: library_path.clone(),
        });

        let app = build_router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/duplicates/trash")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"file_ids": [{}]}}"#, file_id)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["trashed"], 1);

        // File should be moved to .picman-trash/photos/dupe.jpg
        assert!(!photos_dir.join("dupe.jpg").exists());
        assert!(library_path
            .join(".picman-trash/photos/dupe.jpg")
            .exists());

        // DB entry should be gone
        let db = state.db.lock().unwrap();
        assert!(db.get_file_with_path(file_id).unwrap().is_none());
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
