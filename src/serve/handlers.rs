use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};

use crate::db::Database;
use crate::perceptual_hash;
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
                 SELECT f.id, f.filename, f.directory_id, d.path, f.size, f.rating, f.media_type, f.width, f.height
                 FROM files f
                 JOIN directories d ON f.directory_id = d.id
                 WHERE f.directory_id IN (SELECT id FROM descendants)
                 ORDER BY d.path, f.filename
                 LIMIT ?2 OFFSET ?3",
            )?
        } else {
            conn.prepare(
                "SELECT f.id, f.filename, f.directory_id, d.path, f.size, f.rating, f.media_type, f.width, f.height
                 FROM files f
                 JOIN directories d ON f.directory_id = d.id
                 WHERE f.directory_id = ?1
                 ORDER BY f.filename
                 LIMIT ?2 OFFSET ?3",
            )?
        };

        #[allow(clippy::type_complexity)]
        let file_rows: Vec<(i64, String, i64, String, i64, Option<i32>, Option<String>, Option<i32>, Option<i32>)> = stmt
            .query_map(rusqlite::params![dir_id, per_page as i64, offset as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Batch-fetch tags for these files
        let file_ids: Vec<i64> = file_rows.iter().map(|f| f.0).collect();
        let all_file_tags = batch_get_file_tags(conn, &file_ids)?;

        let files: Vec<FileResponse> = file_rows
            .into_iter()
            .map(|(id, filename, directory_id, dir_path, size, rating, media_type, width, height)| {
                let tags = all_file_tags.get(&id).cloned().unwrap_or_default();
                FileResponse {
                    id,
                    filename,
                    directory_id,
                    directory_path: dir_path,
                    size,
                    rating,
                    media_type,
                    width,
                    height,
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
            "SELECT f.id, f.filename, f.directory_id, d.path, f.size, f.rating, f.media_type, f.width, f.height
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
        #[allow(clippy::type_complexity)]
        let file_ids_and_data: Vec<(i64, String, i64, String, i64, Option<i32>, Option<String>, Option<i32>, Option<i32>)> = stmt
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
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        // Batch-fetch tags for these files
        let file_ids: Vec<i64> = file_ids_and_data.iter().map(|f| f.0).collect();
        let all_file_tags = batch_get_file_tags(conn, &file_ids)?;

        let files: Vec<FileResponse> = file_ids_and_data
            .into_iter()
            .map(|(id, filename, directory_id, dir_path, size, rating, media_type, width, height)| {
                let tags = all_file_tags.get(&id).cloned().unwrap_or_default();
                FileResponse {
                    id,
                    filename,
                    directory_id,
                    directory_path: dir_path,
                    size,
                    rating,
                    media_type,
                    width,
                    height,
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

fn batch_get_file_dirs(
    conn: &rusqlite::Connection,
    file_ids: &[i64],
) -> Result<HashMap<i64, String>, rusqlite::Error> {
    if file_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result: HashMap<i64, String> = HashMap::new();

    // Chunk to avoid SQLite variable limit (999)
    for chunk in file_ids.chunks(500) {
        let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!(
            "SELECT f.id, d.path FROM files f
             JOIN directories d ON f.directory_id = d.id
             WHERE f.id IN ({})",
            placeholders
        );

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(chunk.iter()),
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )?;

        for row in rows {
            let (file_id, dir_path) = row?;
            result.insert(file_id, dir_path);
        }
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

// ==================== Duplicates ====================

#[derive(serde::Deserialize)]
pub struct DuplicatesParams {
    #[serde(rename = "type")]
    pub match_type: Option<String>,
    pub threshold: Option<u32>,
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

pub async fn get_duplicates_summary(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DuplicatesParams>,
) -> Result<Json<DuplicatesSummary>, AppError> {
    let threshold = params.threshold.unwrap_or(8);
    let db = state.db.clone();

    let summary = spawn_db(db, move |db| {
        let exact_groups = db.find_duplicates_with_paths()?;
        let exact_files: usize = exact_groups.iter().map(|g| g.files.len()).sum();

        let hashes_raw = db.get_all_perceptual_hashes()?;
        let hashes_u64: Vec<(i64, u64)> = hashes_raw
            .iter()
            .map(|(id, h)| (*id, *h as u64))
            .collect();
        let similar_groups_raw = perceptual_hash::group_by_similarity(&hashes_u64, threshold);

        // Exclude file IDs that are in exact duplicate groups
        let exact_file_ids: std::collections::HashSet<i64> = exact_groups
            .iter()
            .flat_map(|g| g.files.iter().map(|(f, _)| f.id))
            .collect();

        let similar_groups: Vec<&Vec<i64>> = similar_groups_raw
            .iter()
            .filter(|group| {
                // Keep group if it has 2+ files NOT in exact groups
                let non_exact: Vec<&i64> = group
                    .iter()
                    .filter(|id| !exact_file_ids.contains(id))
                    .collect();
                non_exact.len() >= 2
            })
            .collect();

        let similar_files: usize = similar_groups
            .iter()
            .map(|g| {
                g.iter()
                    .filter(|id| !exact_file_ids.contains(id))
                    .count()
            })
            .sum();

        Ok(DuplicatesSummary {
            exact_groups: exact_groups.len(),
            exact_files,
            similar_groups: similar_groups.len(),
            similar_files,
        })
    })
    .await?;

    Ok(Json(summary))
}

pub async fn get_duplicates(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DuplicatesParams>,
) -> Result<Json<DuplicatesResponse>, AppError> {
    let match_type = params.match_type.unwrap_or_else(|| "exact".to_string());
    let threshold = params.threshold.unwrap_or(8);
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200);
    let db = state.db.clone();

    let response = spawn_db(db, move |db| {
        let conn = db.connection();

        match match_type.as_str() {
            "exact" => build_exact_response(db, conn, page, per_page),
            "similar" => build_similar_response(db, conn, threshold, page, per_page),
            _ => Err(anyhow::anyhow!("Invalid type: must be 'exact' or 'similar'")),
        }
    })
    .await?;

    Ok(Json(response))
}

fn build_exact_response(
    db: &Database,
    conn: &rusqlite::Connection,
    page: usize,
    per_page: usize,
) -> anyhow::Result<DuplicatesResponse> {
    let exact_groups = db.find_duplicates_with_paths()?;
    let total_groups = exact_groups.len();

    // Compute folder super-groups from ALL groups before pagination
    let all_group_folders: Vec<(usize, Vec<String>)> = exact_groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let mut dirs: Vec<String> = g.files.iter().map(|(_, dir)| dir.clone()).collect();
            dirs.sort();
            dirs.dedup();
            (i, dirs)
        })
        .collect();
    let folder_super_groups = compute_folder_super_groups(&all_group_folders);

    // Paginate
    let start = (page - 1) * per_page;
    let paged_groups: Vec<_> = exact_groups
        .into_iter()
        .enumerate()
        .skip(start)
        .take(per_page)
        .collect();

    // Batch-fetch tags for all files in paged groups
    let all_file_ids: Vec<i64> = paged_groups
        .iter()
        .flat_map(|(_, g)| g.files.iter().map(|(f, _)| f.id))
        .collect();
    let all_tags = batch_get_file_tags(conn, &all_file_ids)?;

    let mut groups = Vec::new();

    for (group_index, dup_group) in &paged_groups {
        let files: Vec<DuplicateFileResponse> = dup_group
            .files
            .iter()
            .map(|(f, dir_path)| DuplicateFileResponse {
                id: f.id,
                filename: f.filename.clone(),
                directory_path: dir_path.clone(),
                size: f.size,
                width: f.width,
                height: f.height,
                rating: f.rating,
                media_type: f.media_type.clone(),
                tags: all_tags.get(&f.id).cloned().unwrap_or_default(),
            })
            .collect();

        let suggested_keep_id = suggest_keep_id(&files);

        groups.push(DuplicateGroupResponse {
            group_index: *group_index,
            match_type: "exact".to_string(),
            hash: Some(dup_group.hash.clone()),
            max_distance: None,
            files,
            suggested_keep_id,
        });
    }

    Ok(DuplicatesResponse {
        groups,
        total_groups,
        page,
        per_page,
        folder_super_groups,
    })
}

fn build_similar_response(
    db: &Database,
    conn: &rusqlite::Connection,
    threshold: u32,
    page: usize,
    per_page: usize,
) -> anyhow::Result<DuplicatesResponse> {
    // Get exact duplicate file IDs to exclude
    let exact_groups = db.find_duplicates_with_paths()?;
    let exact_file_ids: std::collections::HashSet<i64> = exact_groups
        .iter()
        .flat_map(|g| g.files.iter().map(|(f, _)| f.id))
        .collect();

    let hashes_raw = db.get_all_perceptual_hashes()?;
    let hashes_u64: Vec<(i64, u64)> = hashes_raw
        .iter()
        .map(|(id, h)| (*id, *h as u64))
        .collect();
    let similar_groups_raw = perceptual_hash::group_by_similarity(&hashes_u64, threshold);

    // Filter: keep groups with 2+ non-exact files
    let filtered_groups: Vec<Vec<i64>> = similar_groups_raw
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .filter(|id| !exact_file_ids.contains(id))
                .collect::<Vec<_>>()
        })
        .filter(|group| group.len() >= 2)
        .collect();

    let total_groups = filtered_groups.len();

    // Compute folder super-groups from ALL groups before pagination
    let all_similar_ids: Vec<i64> = filtered_groups.iter().flat_map(|g| g.iter().copied()).collect();
    let file_dir_map = batch_get_file_dirs(conn, &all_similar_ids)?;

    let all_group_folders: Vec<(usize, Vec<String>)> = filtered_groups
        .iter()
        .enumerate()
        .map(|(i, fids)| {
            let mut dirs: Vec<String> = fids
                .iter()
                .filter_map(|id| file_dir_map.get(id).cloned())
                .collect();
            dirs.sort();
            dirs.dedup();
            (i, dirs)
        })
        .collect();
    let folder_super_groups = compute_folder_super_groups(&all_group_folders);

    // Paginate
    let start = (page - 1) * per_page;
    let paged_groups: Vec<_> = filtered_groups
        .into_iter()
        .enumerate()
        .skip(start)
        .take(per_page)
        .collect();

    // Build hash lookup for distance computation
    let hash_map: HashMap<i64, u64> = hashes_u64.iter().copied().collect();

    // Batch fetch all file data
    let all_file_ids: Vec<i64> = paged_groups
        .iter()
        .flat_map(|(_, g)| g.iter().copied())
        .collect();
    let all_tags = batch_get_file_tags(conn, &all_file_ids)?;

    let mut groups = Vec::new();

    for (group_index, file_ids) in &paged_groups {
        let mut files = Vec::new();
        for &file_id in file_ids {
            if let Some((f, dir_path)) = db.get_file_with_path(file_id)? {
                files.push(DuplicateFileResponse {
                    id: f.id,
                    filename: f.filename,
                    directory_path: dir_path,
                    size: f.size,
                    width: f.width,
                    height: f.height,
                    rating: f.rating,
                    media_type: f.media_type,
                    tags: all_tags.get(&f.id).cloned().unwrap_or_default(),
                });
            }
        }

        if files.len() < 2 {
            continue;
        }

        // Compute max Hamming distance within group
        let mut max_distance = 0u32;
        for (i, &a) in file_ids.iter().enumerate() {
            for &b in &file_ids[i + 1..] {
                let ha = hash_map.get(&a).copied().unwrap_or(0);
                let hb = hash_map.get(&b).copied().unwrap_or(0);
                let d = perceptual_hash::hamming_distance(ha, hb);
                if d > max_distance {
                    max_distance = d;
                }
            }
        }

        let suggested_keep_id = suggest_keep_id(&files);

        groups.push(DuplicateGroupResponse {
            group_index: *group_index,
            match_type: "similar".to_string(),
            hash: None,
            max_distance: Some(max_distance),
            files,
            suggested_keep_id,
        });
    }

    Ok(DuplicatesResponse {
        groups,
        total_groups,
        page,
        per_page,
        folder_super_groups,
    })
}

/// Pick the best file to keep: highest resolution (width×height), fallback to largest size.
fn suggest_keep_id(files: &[DuplicateFileResponse]) -> i64 {
    files
        .iter()
        .max_by_key(|f| {
            let pixels = f
                .width
                .unwrap_or(0)
                .saturating_mul(f.height.unwrap_or(0)) as i64;
            (pixels, f.size)
        })
        .map(|f| f.id)
        .unwrap_or(0)
}

/// Compute folder super-groups: pairs of folders that appear together in 2+ groups.
fn compute_folder_super_groups(
    all_group_folders: &[(usize, Vec<String>)],
) -> Vec<FolderSuperGroup> {
    let mut pair_map: HashMap<(String, String), Vec<usize>> = HashMap::new();

    for (group_index, dirs) in all_group_folders {
        if dirs.len() == 2 {
            let pair = if dirs[0] <= dirs[1] {
                (dirs[0].clone(), dirs[1].clone())
            } else {
                (dirs[1].clone(), dirs[0].clone())
            };
            pair_map.entry(pair).or_default().push(*group_index);
        }
    }

    pair_map
        .into_iter()
        .filter(|(_, indices)| indices.len() >= 2)
        .map(|((a, b), indices)| FolderSuperGroup {
            folders: vec![a, b],
            group_indices: indices,
        })
        .collect()
}

pub async fn trash_files(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TrashFilesRequest>,
) -> Result<Json<TrashFilesResponse>, AppError> {
    if body.file_ids.is_empty() {
        return Ok(Json(TrashFilesResponse {
            trashed: 0,
            errors: vec![],
        }));
    }

    let (trashed, errors) = execute_trash(
        state.db.clone(),
        state.library_path.clone(),
        body.file_ids,
    )
    .await?;

    Ok(Json(TrashFilesResponse { trashed, errors }))
}

/// Reusable trash logic: resolve paths → move files → delete from DB.
/// Returns (trashed_count, errors).
async fn execute_trash(
    db: Arc<Mutex<Database>>,
    library_path: PathBuf,
    file_ids: Vec<i64>,
) -> Result<(usize, Vec<TrashErrorResponse>), AppError> {
    if file_ids.is_empty() {
        return Ok((0, vec![]));
    }

    // Phase 1: resolve file paths from DB
    let file_paths: Vec<(i64, PathBuf, String, String)> = spawn_db(db.clone(), {
        let library_path = library_path.clone();
        let file_ids = file_ids.clone();
        move |db| {
            let mut paths = Vec::new();
            for file_id in &file_ids {
                if let Some((file, dir_path)) = db.get_file_with_path(*file_id)? {
                    let full_path = if dir_path.is_empty() {
                        library_path.join(&file.filename)
                    } else {
                        library_path.join(&dir_path).join(&file.filename)
                    };
                    paths.push((*file_id, full_path, dir_path, file.filename));
                }
            }
            Ok(paths)
        }
    })
    .await?;

    // Phase 2: move files + delete thumbnails (no DB lock held)
    let move_results: Vec<(i64, Result<(), String>)> =
        tokio::task::spawn_blocking({
            let library_path = library_path.clone();
            move || {
                let trash_root = library_path.join(".picman-trash");
                let mut results = Vec::new();

                for (file_id, full_path, dir_path, filename) in &file_paths {
                    let result = (|| -> Result<(), String> {
                        if !full_path.exists() {
                            return Err(format!("File not found: {}", full_path.display()));
                        }

                        // Compute thumbnail paths BEFORE moving (needs fs::metadata)
                        let thumb_path = thumbnails::get_thumbnail_path(full_path);
                        let video_thumb_path = thumbnails::get_video_thumbnail_path(full_path);
                        let web_thumb_path = thumbnails::get_web_thumbnail_path(full_path);

                        // Compute trash destination
                        let trash_dir = if dir_path.is_empty() {
                            trash_root.clone()
                        } else {
                            trash_root.join(dir_path)
                        };
                        let mut trash_path = trash_dir.join(filename);

                        // Handle name conflicts
                        if trash_path.exists() {
                            let stem = std::path::Path::new(filename)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let ext = std::path::Path::new(filename)
                                .extension()
                                .map(|e| format!(".{}", e.to_string_lossy()))
                                .unwrap_or_default();
                            for i in 2.. {
                                trash_path = trash_dir.join(format!("{}_{}{}", stem, i, ext));
                                if !trash_path.exists() {
                                    break;
                                }
                            }
                        }

                        std::fs::create_dir_all(&trash_dir)
                            .map_err(|e| format!("Failed to create trash dir: {}", e))?;
                        std::fs::rename(full_path, &trash_path)
                            .map_err(|e| format!("Failed to move file: {}", e))?;

                        // Delete thumbnails (ignore errors — may not exist)
                        if let Some(p) = thumb_path {
                            let _ = std::fs::remove_file(p);
                        }
                        if let Some(p) = video_thumb_path {
                            let _ = std::fs::remove_file(p);
                        }
                        if let Some(p) = web_thumb_path {
                            let _ = std::fs::remove_file(p);
                        }

                        Ok(())
                    })();

                    results.push((*file_id, result));
                }

                results
            }
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Phase 3: delete successfully moved files from DB
    let successfully_moved: Vec<i64> = move_results
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(id, _)| *id)
        .collect();

    let errors: Vec<TrashErrorResponse> = move_results
        .iter()
        .filter_map(|(id, r)| {
            r.as_ref().err().map(|e| TrashErrorResponse {
                file_id: *id,
                error: e.clone(),
            })
        })
        .collect();

    if !successfully_moved.is_empty() {
        spawn_db(db, move |db| {
            db.begin_transaction()?;
            for file_id in &successfully_moved {
                db.delete_file(*file_id)?;
            }
            db.commit()?;
            Ok(())
        })
        .await?;
    }

    let trashed = move_results.iter().filter(|(_, r)| r.is_ok()).count();

    Ok((trashed, errors))
}

pub async fn trash_folder_rule(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TrashFolderRuleRequest>,
) -> Result<Json<TrashFolderRuleResponse>, AppError> {
    let db = state.db.clone();
    let keep_folder = body.keep_folder.clone();
    let trash_folder = body.trash_folder.clone();
    let match_type = body.match_type.clone();
    let threshold = body.threshold.unwrap_or(8);

    // Collect file IDs to trash from ALL matching groups
    let (file_ids_to_trash, groups_resolved) = spawn_db(db.clone(), move |db| {
        let conn = db.connection();

        match match_type.as_str() {
            "exact" => {
                let exact_groups = db.find_duplicates_with_paths()?;
                let mut to_trash = Vec::new();
                let mut resolved = 0usize;

                for group in &exact_groups {
                    let has_keep = group.files.iter().any(|(_, dir)| *dir == keep_folder);
                    let has_trash = group.files.iter().any(|(_, dir)| *dir == trash_folder);

                    if has_keep && has_trash {
                        for (f, dir) in &group.files {
                            if *dir == trash_folder {
                                to_trash.push(f.id);
                            }
                        }
                        resolved += 1;
                    }
                }

                Ok((to_trash, resolved))
            }
            "similar" => {
                // Get exact duplicate file IDs to exclude
                let exact_groups = db.find_duplicates_with_paths()?;
                let exact_file_ids: std::collections::HashSet<i64> = exact_groups
                    .iter()
                    .flat_map(|g| g.files.iter().map(|(f, _)| f.id))
                    .collect();

                let hashes_raw = db.get_all_perceptual_hashes()?;
                let hashes_u64: Vec<(i64, u64)> = hashes_raw
                    .iter()
                    .map(|(id, h)| (*id, *h as u64))
                    .collect();
                let similar_groups_raw =
                    perceptual_hash::group_by_similarity(&hashes_u64, threshold);

                // Filter: keep groups with 2+ non-exact files
                let filtered_groups: Vec<Vec<i64>> = similar_groups_raw
                    .into_iter()
                    .map(|group| {
                        group
                            .into_iter()
                            .filter(|id| !exact_file_ids.contains(id))
                            .collect::<Vec<_>>()
                    })
                    .filter(|group| group.len() >= 2)
                    .collect();

                // Get dir paths for all files
                let all_ids: Vec<i64> =
                    filtered_groups.iter().flat_map(|g| g.iter().copied()).collect();
                let dir_map = batch_get_file_dirs(conn, &all_ids)?;

                let mut to_trash = Vec::new();
                let mut resolved = 0usize;

                for group in &filtered_groups {
                    let has_keep = group
                        .iter()
                        .any(|id| dir_map.get(id).map(|d| d.as_str()) == Some(&keep_folder));
                    let has_trash = group
                        .iter()
                        .any(|id| dir_map.get(id).map(|d| d.as_str()) == Some(&trash_folder));

                    if has_keep && has_trash {
                        for id in group {
                            if dir_map.get(id).map(|d| d.as_str()) == Some(&trash_folder) {
                                to_trash.push(*id);
                            }
                        }
                        resolved += 1;
                    }
                }

                Ok((to_trash, resolved))
            }
            _ => Err(anyhow::anyhow!("Invalid match_type: must be 'exact' or 'similar'")),
        }
    })
    .await?;

    let (trashed, errors) = execute_trash(
        db,
        state.library_path.clone(),
        file_ids_to_trash,
    )
    .await?;

    Ok(Json(TrashFolderRuleResponse {
        trashed,
        groups_resolved,
        errors,
    }))
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
