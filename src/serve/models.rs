use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Serialize)]
pub struct DirectoryResponse {
    pub id: i64,
    pub path: String,
    pub parent_id: Option<i64>,
    pub rating: Option<i32>,
    pub tags: Vec<String>,
    pub file_count: usize,
}

#[derive(Serialize)]
pub struct FileResponse {
    pub id: i64,
    pub filename: String,
    pub directory_id: i64,
    pub directory_path: String,
    pub size: i64,
    pub rating: Option<i32>,
    pub media_type: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Serialize)]
pub struct TagResponse {
    pub name: String,
    pub file_count: i64,
    pub directory_count: i64,
}

#[derive(Serialize)]
pub struct PaginatedFiles {
    pub files: Vec<FileResponse>,
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
}

#[derive(Deserialize)]
pub struct SetRatingRequest {
    pub rating: Option<i32>,
}

#[derive(Deserialize)]
pub struct AddTagRequest {
    pub tag: String,
}

#[derive(Serialize)]
pub struct DirectoryMetaResponse {
    pub rating: Option<i32>,
    pub tags: Vec<String>,
}
