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
    pub width: Option<i32>,
    pub height: Option<i32>,
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

// ==================== Duplicates ====================

#[derive(Serialize)]
pub struct DuplicateFileResponse {
    pub id: i64,
    pub filename: String,
    pub directory_path: String,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub rating: Option<i32>,
    pub media_type: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Serialize)]
pub struct DuplicateGroupResponse {
    pub group_index: usize,
    pub match_type: String,
    pub hash: Option<String>,
    pub max_distance: Option<u32>,
    pub files: Vec<DuplicateFileResponse>,
    pub suggested_keep_id: i64,
}

#[derive(Serialize)]
pub struct FolderSuperGroup {
    pub folders: Vec<String>,
    pub group_indices: Vec<usize>,
}

#[derive(Serialize)]
pub struct DuplicatesResponse {
    pub groups: Vec<DuplicateGroupResponse>,
    pub total_groups: usize,
    pub page: usize,
    pub per_page: usize,
    pub folder_super_groups: Vec<FolderSuperGroup>,
}

#[derive(Serialize)]
pub struct DuplicatesSummary {
    pub exact_groups: usize,
    pub exact_files: usize,
    pub similar_groups: usize,
    pub similar_files: usize,
}

#[derive(Deserialize)]
pub struct TrashFilesRequest {
    pub file_ids: Vec<i64>,
}

#[derive(Serialize)]
pub struct TrashFilesResponse {
    pub trashed: usize,
    pub errors: Vec<TrashErrorResponse>,
}

#[derive(Serialize)]
pub struct TrashErrorResponse {
    pub file_id: i64,
    pub error: String,
}
