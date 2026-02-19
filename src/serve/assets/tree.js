// Directory tree rendering, navigation, and breadcrumb.

import { state } from './state.js';
import { pushUrl } from './router.js';
import { loadFiles } from './api.js';
import { renderGrid, renderFileCount } from './grid.js';
import { renderDirRating, renderDirTags } from './tags.js';

function getRootDirectories() {
    return state.directories.filter(d => d.parent_id === null);
}

function getChildren(parentId) {
    return state.directories.filter(d => d.parent_id === parentId);
}

function hasChildren(dirId) {
    return state.directories.some(d => d.parent_id === dirId);
}

function getRecursiveFileCount(dirId) {
    let count = state.dirMap.get(dirId)?.file_count || 0;
    for (const d of state.directories) {
        if (d.parent_id === dirId) {
            count += getRecursiveFileCount(d.id);
        }
    }
    return count;
}

function getTotalFileCount() {
    return state.directories.reduce((sum, d) => sum + (d.file_count || 0), 0);
}

export function renderDirectoryTree() {
    const container = document.getElementById("directory-tree");
    container.innerHTML = "";

    const rootEl = document.createElement("div");
    rootEl.className = "dir-item" + (state.selectedDirId === "root" ? " selected" : "");
    rootEl.style.paddingLeft = "8px";

    const rootToggle = document.createElement("span");
    rootToggle.className = "dir-toggle";

    const rootName = document.createElement("span");
    rootName.className = "dir-name";
    rootName.textContent = "All";

    const rootCount = document.createElement("span");
    rootCount.className = "dir-count";
    const totalFiles = getTotalFileCount();
    if (totalFiles > 0) rootCount.textContent = totalFiles;

    rootEl.appendChild(rootToggle);
    rootEl.appendChild(rootName);
    rootEl.appendChild(rootCount);
    rootEl.addEventListener("click", () => selectDirectory("root"));
    container.appendChild(rootEl);

    const roots = getRootDirectories();
    for (const dir of roots) {
        renderDirNode(container, dir, 0);
    }
}

function renderDirNode(container, dir, depth) {
    const el = document.createElement("div");
    el.className = "dir-item" + (dir.id === state.selectedDirId ? " selected" : "");
    el.style.paddingLeft = (8 + depth * 16) + "px";

    const hasKids = hasChildren(dir.id);
    const expanded = state.expandedDirs.has(dir.id);

    const toggle = document.createElement("span");
    toggle.className = "dir-toggle";
    if (hasKids) {
        toggle.classList.add("icon");
        toggle.textContent = expanded ? "expand_more" : "chevron_right";
    }

    const name = document.createElement("span");
    name.className = "dir-name";
    name.textContent = dirDisplayName(dir);

    const count = document.createElement("span");
    count.className = "dir-count";
    const total = getRecursiveFileCount(dir.id);
    if (total > 0) count.textContent = total;

    el.appendChild(toggle);
    el.appendChild(name);
    el.appendChild(count);

    el.addEventListener("click", (e) => {
        e.stopPropagation();
        selectDirectory(dir.id);
    });

    if (hasKids) {
        toggle.addEventListener("click", (e) => {
            e.stopPropagation();
            toggleExpand(dir.id);
        });
    }

    container.appendChild(el);

    if (expanded && hasKids) {
        const children = getChildren(dir.id);
        for (const child of children) {
            renderDirNode(container, child, depth + 1);
        }
    }
}

function dirDisplayName(dir) {
    if (!dir.path) return "(root)";
    const parts = dir.path.split("/");
    return parts[parts.length - 1];
}

export async function selectDirectory(dirId, { recursive = true, updateUrl = true } = {}) {
    state.selectedDirId = dirId;
    state.recursive = recursive;
    state.useFilteredEndpoint = dirId === "root";
    state.currentPage = 1;
    state.currentFiles = [];

    // Clear filters on directory navigation to avoid stale params in URL
    state.ratingFilter = "";
    state.tagFilter = "";
    document.getElementById("rating-filter").value = "";

    if (updateUrl) pushUrl();

    if (dirId !== "root" && hasChildren(dirId) && !state.expandedDirs.has(dirId)) {
        state.expandedDirs.add(dirId);
    }

    renderDirectoryTree();
    renderBreadcrumb();
    renderDirRating();
    renderDirTags();
    renderFileCount();
    document.getElementById("grid").innerHTML = '<div class="loading">Loading</div>';

    const loaded = await loadFiles(1);
    if (loaded) {
        renderGrid(true);
        renderFileCount();
    }
}

function toggleExpand(dirId) {
    if (state.expandedDirs.has(dirId)) {
        state.expandedDirs.delete(dirId);
    } else {
        state.expandedDirs.add(dirId);
    }
    renderDirectoryTree();
}

export function renderBreadcrumb() {
    const container = document.getElementById("breadcrumb");
    container.innerHTML = "";

    if (state.selectedDirId === "root") {
        const label = document.createElement("span");
        label.className = "current";
        label.textContent = "All";
        container.appendChild(label);
        return;
    }

    if (state.useFilteredEndpoint) {
        const label = document.createElement("span");
        label.className = "current";
        const parts = [];
        if (state.ratingFilter) parts.push("★" + state.ratingFilter + "+");
        if (state.tagFilter) parts.push("#" + state.tagFilter);
        label.textContent = "Filtered: " + (parts.join(", ") || "All files");
        container.appendChild(label);
        return;
    }

    if (state.selectedDirId === null) return;

    const dir = state.dirMap.get(state.selectedDirId);
    if (!dir) return;

    const chain = [];
    let current = dir;
    while (current) {
        chain.unshift(current);
        current = current.parent_id ? state.dirMap.get(current.parent_id) : null;
    }

    chain.forEach((d, i) => {
        if (i > 0) {
            const sep = document.createElement("span");
            sep.className = "separator";
            sep.textContent = "/";
            container.appendChild(sep);
        }

        const span = document.createElement("span");
        span.textContent = dirDisplayName(d);

        if (i === chain.length - 1) {
            span.className = "current";
        } else {
            span.addEventListener("click", () => selectDirectory(d.id));
        }

        container.appendChild(span);
    });
}
