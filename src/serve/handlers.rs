use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};

use crate::db::Database;
use crate::thumbnails;

use super::models::*;
use super::{AppState, Assets};

// ==================== Health ====================

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

// ==================== Directories ====================

pub async fn get_directories(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DirectoryResponse>>, AppError> {
    let db = state.db.clone();
    let dirs = spawn_db(db.clone(), |db| {
        let dirs = db.get_all_directories()?;
        let dir_tags = db.get_all_directory_tags()?;
        let files = db.get_all_files()?;

        // Count files per directory
        let mut file_counts: HashMap<i64, usize> = HashMap::new();
        for f in &files {
            *file_counts.entry(f.directory_id).or_default() += 1;
        }

        let result: Vec<DirectoryResponse> = dirs
            .into_iter()
            .map(|d| {
                let tags = dir_tags
                    .get(&d.id)
                    .cloned()
                    .unwrap_or_default();
                let file_count = file_counts.get(&d.id).copied().unwrap_or(0);
                DirectoryResponse {
                    id: d.id,
                    path: d.path,
                    parent_id: d.parent_id,
                    rating: d.rating,
                    tags,
                    file_count,
                }
            })
            .collect();

        Ok(result)
    })
    .await?;

    Ok(Json(dirs))
}

// ==================== Directory Files ====================

#[derive(serde::Deserialize)]
pub struct PaginationParams {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
    pub recursive: Option<bool>,
}

pub async fn get_directory_files(
    State(state): State<Arc<AppState>>,
    Path(dir_id): Path<i64>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedFiles>, AppError> {
    let db = state.db.clone();
    let result = spawn_db(db, move |db| {
        let conn = db.connection();

        let page = params.page.unwrap_or(1).max(1);
        let per_page = params.per_page.unwrap_or(100).min(500);
        let offset = (page - 1) * per_page;
        let recursive = params.recursive.unwrap_or(true);

        let total: usize = if recursive {
            conn.query_row(
                "WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM directories WHERE id = ?1
                     UNION ALL
                     SELECT d.id FROM directories d
                     JOIN descendants dd ON d.parent_id = dd.id
                 )
                 SELECT COUNT(*) FROM files f
                 WHERE f.directory_id IN (SELECT id FROM descendants)",
                [dir_id],
                |row| row.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM files WHERE directory_id = ?1",
                [dir_id],
                |row| row.get(0),
            )?
        };

        let mut stmt = if recursive {
            conn.prepare(
                "WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM directories WHERE id = ?1
                     UNION ALL
                     SELECT d.id FROM directories d
                     JOIN descendants dd ON d.parent_id = dd.id
                 )
                 SELECT f.id, f.filename, f.directory_id, d.path, f.size, f.rating, f.media_type
                 FROM files f
                 JOIN directories d ON f.directory_id = d.id
                 WHERE f.directory_id IN (SELECT id FROM descendants)
                 ORDER BY d.path, f.filename
                 LIMIT ?2 OFFSET ?3",
            )?
        } else {
            conn.prepare(
                "SELECT f.id, f.filename, f.directory_id, d.path, f.size, f.rating, f.media_type
                 FROM files f
                 JOIN directories d ON f.directory_id = d.id
                 WHERE f.directory_id = ?1
                 ORDER BY f.filename
                 LIMIT ?2 OFFSET ?3",
            )?
        };

        let file_rows: Vec<(i64, String, i64, String, i64, Option<i32>, Option<String>)> = stmt
            .query_map(rusqlite::params![dir_id, per_page as i64, offset as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Batch-fetch tags for these files
        let file_ids: Vec<i64> = file_rows.iter().map(|f| f.0).collect();
        let all_file_tags = batch_get_file_tags(conn, &file_ids)?;

        let files: Vec<FileResponse> = file_rows
            .into_iter()
            .map(|(id, filename, directory_id, dir_path, size, rating, media_type)| {
                let tags = all_file_tags.get(&id).cloned().unwrap_or_default();
                FileResponse {
                    id,
                    filename,
                    directory_id,
                    directory_path: dir_path,
                    size,
                    rating,
                    media_type,
                    tags,
                }
            })
            .collect();

        Ok(PaginatedFiles {
            files,
            total,
            page,
            per_page,
        })
    })
    .await?;

    Ok(Json(result))
}

// ==================== Tags ====================

pub async fn get_tags(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<TagResponse>>, AppError> {
    let db = state.db.clone();
    let tags = spawn_db(db, |db| {
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT t.name,
                    (SELECT COUNT(*) FROM file_tags ft WHERE ft.tag_id = t.id) as file_count,
                    (SELECT COUNT(*) FROM directory_tags dt WHERE dt.tag_id = t.id) as dir_count
             FROM tags t
             ORDER BY t.name",
        )?;

        let tags: Vec<TagResponse> = stmt
            .query_map([], |row| {
                Ok(TagResponse {
                    name: row.get(0)?,
                    file_count: row.get(1)?,
                    directory_count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    })
    .await?;

    Ok(Json(tags))
}

// ==================== Directory Mutations ====================

pub async fn set_directory_rating(
    State(state): State<Arc<AppState>>,
    Path(dir_id): Path<i64>,
    Json(body): Json<SetRatingRequest>,
) -> Result<Json<DirectoryMetaResponse>, AppError> {
    if let Some(r) = body.rating {
        if !(1..=5).contains(&r) {
            return Err(AppError::BadRequest("Rating must be between 1 and 5".into()));
        }
    }

    let db = state.db.clone();
    let meta = spawn_db(db, move |db| {
        db.set_directory_rating(dir_id, body.rating)?;
        let rating = db.get_directory(dir_id)?.map(|d| d.rating).unwrap_or(None);
        let tags = db.get_directory_tags(dir_id)?;
        Ok(DirectoryMetaResponse { rating, tags })
    })
    .await?;

    Ok(Json(meta))
}

pub async fn add_directory_tag(
    State(state): State<Arc<AppState>>,
    Path(dir_id): Path<i64>,
    Json(body): Json<AddTagRequest>,
) -> Result<Json<DirectoryMetaResponse>, AppError> {
    let tag = body.tag.trim().to_lowercase();
    if tag.is_empty() {
        return Err(AppError::BadRequest("Tag name cannot be empty".into()));
    }

    let db = state.db.clone();
    let meta = spawn_db(db, move |db| {
        db.add_directory_tag(dir_id, &tag)?;
        let rating = db.get_directory(dir_id)?.map(|d| d.rating).unwrap_or(None);
        let tags = db.get_directory_tags(dir_id)?;
        Ok(DirectoryMetaResponse { rating, tags })
    })
    .await?;

    Ok(Json(meta))
}

pub async fn remove_directory_tag(
    State(state): State<Arc<AppState>>,
    Path((dir_id, tag_name)): Path<(i64, String)>,
) -> Result<Json<DirectoryMetaResponse>, AppError> {
    let db = state.db.clone();
    let meta = spawn_db(db, move |db| {
        db.remove_directory_tag(dir_id, &tag_name)?;
        let rating = db.get_directory(dir_id)?.map(|d| d.rating).unwrap_or(None);
        let tags = db.get_directory_tags(dir_id)?;
        Ok(DirectoryMetaResponse { rating, tags })
    })
    .await?;

    Ok(Json(meta))
}

// ==================== Filtered Files ====================

#[derive(serde::Deserialize)]
pub struct FileFilterParams {
    pub rating: Option<i32>,
    pub tag: Option<String>,
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

pub async fn get_filtered_files(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FileFilterParams>,
) -> Result<Json<PaginatedFiles>, AppError> {
    let db = state.db.clone();
    let result = spawn_db(db, move |db| {
        let conn = db.connection();

        let mut conditions = Vec::new();
        let mut sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(rating) = params.rating {
            conditions.push(format!("f.rating >= ?{}", sql_params.len() + 1));
            sql_params.push(Box::new(rating));
        }

        if let Some(ref tag) = params.tag {
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM file_tags ft JOIN tags t ON ft.tag_id = t.id WHERE ft.file_id = f.id AND t.name = ?{})",
                sql_params.len() + 1
            ));
            sql_params.push(Box::new(tag.clone()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Get total count
        let count_sql = format!(
            "SELECT COUNT(*) FROM files f {}",
            where_clause
        );
        let total: usize = conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(sql_params.iter().map(|p| p.as_ref())),
            |row| row.get(0),
        )?;

        let page = params.page.unwrap_or(1).max(1);
        let per_page = params.per_page.unwrap_or(100).min(500);
        let offset = (page - 1) * per_page;

        let query = format!(
            "SELECT f.id, f.filename, f.directory_id, d.path, f.size, f.rating, f.media_type
             FROM files f
             JOIN directories d ON f.directory_id = d.id
             {}
             ORDER BY d.path, f.filename
             LIMIT ?{} OFFSET ?{}",
            where_clause,
            sql_params.len() + 1,
            sql_params.len() + 2,
        );

        sql_params.push(Box::new(per_page as i64));
        sql_params.push(Box::new(offset as i64));

        let mut stmt = conn.prepare(&query)?;
        let file_ids_and_data: Vec<(i64, String, i64, String, i64, Option<i32>, Option<String>)> = stmt
            .query_map(
                rusqlite::params_from_iter(sql_params.iter().map(|p| p.as_ref())),
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        // Batch-fetch tags for these files
        let file_ids: Vec<i64> = file_ids_and_data.iter().map(|f| f.0).collect();
        let all_file_tags = batch_get_file_tags(conn, &file_ids)?;

        let files: Vec<FileResponse> = file_ids_and_data
            .into_iter()
            .map(|(id, filename, directory_id, dir_path, size, rating, media_type)| {
                let tags = all_file_tags.get(&id).cloned().unwrap_or_default();
                FileResponse {
                    id,
                    filename,
                    directory_id,
                    directory_path: dir_path,
                    size,
                    rating,
                    media_type,
                    tags,
                }
            })
            .collect();

        Ok(PaginatedFiles {
            files,
            total,
            page,
            per_page,
        })
    })
    .await?;

    Ok(Json(result))
}

fn batch_get_file_tags(
    conn: &rusqlite::Connection,
    file_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>, rusqlite::Error> {
    if file_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders: String = file_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let query = format!(
        "SELECT ft.file_id, t.name FROM file_tags ft
         JOIN tags t ON ft.tag_id = t.id
         WHERE ft.file_id IN ({})
         ORDER BY ft.file_id, t.name",
        placeholders
    );

    let mut stmt = conn.prepare(&query)?;
    let mut result: HashMap<i64, Vec<String>> = HashMap::new();

    let rows = stmt.query_map(
        rusqlite::params_from_iter(file_ids.iter()),
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;

    for row in rows {
        let (file_id, tag_name) = row?;
        result.entry(file_id).or_default().push(tag_name);
    }

    Ok(result)
}

// ==================== Thumbnail Serving ====================

pub async fn serve_web_thumbnail(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<i64>,
) -> Result<Response, AppError> {
    let library_path = state.library_path.clone();
    let db = state.db.clone();

    let file_path = spawn_db(db, move |db| resolve_file_path(db, &library_path, file_id))
        .await?;

    let Some(file_path) = file_path else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let thumb_path = thumbnails::get_preview_path_for_file(&file_path).map(|(path, _)| path);
    serve_cached_image(thumb_path).await
}

pub async fn serve_preview(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<i64>,
) -> Result<Response, AppError> {
    let library_path = state.library_path.clone();
    let db = state.db.clone();

    let file_path = spawn_db(db, move |db| resolve_file_path(db, &library_path, file_id))
        .await?;

    let Some(file_path) = file_path else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let preview = thumbnails::get_preview_path_for_file(&file_path);
    match preview {
        Some((path, _)) => serve_cached_image(Some(path)).await,
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

pub async fn serve_dir_preview(
    State(_state): State<Arc<AppState>>,
    Path(dir_id): Path<i64>,
) -> Result<Response, AppError> {
    let path = thumbnails::get_cached_dir_preview(dir_id);
    serve_cached_image(path).await
}

async fn serve_cached_image(path: Option<PathBuf>) -> Result<Response, AppError> {
    let Some(path) = path else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    if !path.exists() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CACHE_CONTROL, "max-age=86400")
        .body(Body::from(bytes))
        .unwrap())
}

// ==================== Original File Serving ====================

pub async fn serve_original(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response, AppError> {
    let requested = state.library_path.join(&path);

    // Path traversal protection: canonicalize and verify prefix
    let canonical = requested
        .canonicalize()
        .map_err(|_| AppError::NotFound)?;

    if !canonical.starts_with(&state.library_path) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }

    if !canonical.is_file() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    let bytes = tokio::fs::read(&canonical)
        .await
        .map_err(|_| AppError::NotFound)?;

    let content_type = mime_guess::from_path(&canonical)
        .first_or_octet_stream()
        .to_string();

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "max-age=86400")
        .body(Body::from(bytes))
        .unwrap())
}

// ==================== Embedded Assets ====================

pub async fn serve_embedded_asset(
    req: axum::extract::Request,
) -> Result<Response, AppError> {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let content_type = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(content.data.to_vec()))
                .unwrap())
        }
        None => {
            // SPA fallback: serve index.html for unknown routes
            match Assets::get("index.html") {
                Some(content) => Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(content.data.to_vec()))
                    .unwrap()),
                None => Ok(StatusCode::NOT_FOUND.into_response()),
            }
        }
    }
}

// ==================== Helpers ====================

fn resolve_file_path(db: &Database, library_path: &std::path::Path, file_id: i64) -> anyhow::Result<Option<PathBuf>> {
    let conn = db.connection();

    let result: Option<(String, String)> = conn
        .query_row(
            "SELECT f.filename, d.path FROM files f JOIN directories d ON f.directory_id = d.id WHERE f.id = ?1",
            [file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let Some((filename, dir_path)) = result else {
        return Ok(None);
    };

    let path = if dir_path.is_empty() {
        library_path.join(&filename)
    } else {
        library_path.join(&dir_path).join(&filename)
    };

    Ok(Some(path))
}

async fn spawn_db<F, T>(db: Arc<Mutex<Database>>, f: F) -> Result<T, AppError>
where
    F: FnOnce(&Database) -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let db = db.lock().map_err(|_| AppError::Internal("Database lock poisoned".into()))?;
        f(&db).map_err(|e| AppError::Internal(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

// ==================== Error Type ====================

pub enum AppError {
    NotFound,
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => StatusCode::NOT_FOUND.into_response(),
            AppError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, msg).into_response()
            }
            AppError::Internal(msg) => {
                eprintln!("Internal error: {}", msg);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
