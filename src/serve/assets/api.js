// HTTP helpers and data fetching.
// Updates state but never calls render functions.

import { state } from './state.js';

async function fetchJson(url) {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.json();
}

async function apiRequest(url, method, body) {
    const opts = { method, headers: {} };
    if (body !== undefined) {
        opts.headers["Content-Type"] = "application/json";
        opts.body = JSON.stringify(body);
    }
    const res = await fetch(url, opts);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.json();
}

export async function loadDirectories() {
    state.directories = await fetchJson("/api/directories");
    state.dirMap.clear();
    for (const d of state.directories) {
        state.dirMap.set(d.id, d);
    }
}

export async function loadTags() {
    state.tags = await fetchJson("/api/tags");
}

// Fetches files based on current state. Updates state.currentFiles/totalFiles/currentPage.
// Returns true if data was loaded, false if the request was stale or skipped.
export async function loadFiles(page = 1) {
    const generation = ++state.loadGeneration;
    state.loading = true;

    try {
        let data;
        if (state.useFilteredEndpoint) {
            const params = new URLSearchParams();
            if (state.ratingFilter) params.set("rating", state.ratingFilter);
            if (state.tagFilter) params.set("tag", state.tagFilter);
            params.set("page", page);
            params.set("per_page", state.perPage);
            data = await fetchJson(`/api/files?${params}`);
        } else if (state.selectedDirId !== null) {
            const params = new URLSearchParams();
            params.set("page", page);
            params.set("per_page", state.perPage);
            if (!state.recursive) params.set("recursive", "false");
            data = await fetchJson(`/api/directories/${state.selectedDirId}/files?${params}`);
        } else {
            return false;
        }

        if (generation !== state.loadGeneration) return false;

        if (page === 1) {
            state.currentFiles = data.files;
        } else {
            state.currentFiles = state.currentFiles.concat(data.files);
        }
        state.totalFiles = data.total;
        state.currentPage = data.page;
        return true;
    } finally {
        if (generation === state.loadGeneration) {
            state.loading = false;
        }
    }
}

export async function setDirRating(dirId, value) {
    return apiRequest(`/api/directories/${dirId}/rating`, "PUT", { rating: value });
}

export async function addDirTag(dirId, name) {
    return apiRequest(`/api/directories/${dirId}/tags`, "POST", { tag: name });
}

export async function removeDirTag(dirId, tagName) {
    return apiRequest(
        `/api/directories/${dirId}/tags/${encodeURIComponent(tagName)}`,
        "DELETE"
    );
}
