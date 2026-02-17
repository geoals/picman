// ==================== State ====================

const state = {
    directories: [],       // All directories from API
    dirMap: new Map(),     // id -> directory
    tags: [],              // All tags from API
    selectedDirId: null,   // Currently selected directory
    expandedDirs: new Set(), // Expanded directory IDs
    currentFiles: [],      // Files currently displayed
    totalFiles: 0,
    currentPage: 1,
    perPage: 100,
    loading: false,
    ratingFilter: "",
    tagFilter: "",
    lightboxIndex: -1,     // -1 = closed
    useFilteredEndpoint: false, // Whether to use /api/files instead of /api/directories/:id/files
};

// ==================== API ====================

async function fetchJson(url) {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.json();
}

async function loadDirectories() {
    state.directories = await fetchJson("/api/directories");
    state.dirMap.clear();
    for (const d of state.directories) {
        state.dirMap.set(d.id, d);
    }
}

async function loadTags() {
    state.tags = await fetchJson("/api/tags");
}

async function loadFiles(page = 1) {
    if (state.loading) return;
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
            data = await fetchJson(`/api/directories/${state.selectedDirId}/files?${params}`);
        } else {
            state.loading = false;
            return;
        }

        if (page === 1) {
            state.currentFiles = data.files;
        } else {
            state.currentFiles = state.currentFiles.concat(data.files);
        }
        state.totalFiles = data.total;
        state.currentPage = data.page;

        renderGrid(page === 1);
        renderFileCount();
    } finally {
        state.loading = false;
    }
}

// ==================== Directory Tree ====================

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

function renderDirectoryTree() {
    const container = document.getElementById("directory-tree");
    container.innerHTML = "";

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
    toggle.textContent = hasKids ? (expanded ? "▾" : "▸") : "";

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

function selectDirectory(dirId) {
    state.selectedDirId = dirId;
    state.useFilteredEndpoint = false;
    state.currentPage = 1;
    state.currentFiles = [];

    // Auto-expand when selecting
    if (hasChildren(dirId) && !state.expandedDirs.has(dirId)) {
        state.expandedDirs.add(dirId);
    }

    renderDirectoryTree();
    renderBreadcrumb();
    loadFiles(1);
}

function toggleExpand(dirId) {
    if (state.expandedDirs.has(dirId)) {
        state.expandedDirs.delete(dirId);
    } else {
        state.expandedDirs.add(dirId);
    }
    renderDirectoryTree();
}

// ==================== Breadcrumb ====================

function renderBreadcrumb() {
    const container = document.getElementById("breadcrumb");
    container.innerHTML = "";

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

    // Build path chain
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

// ==================== Photo Grid ====================

function renderGrid(replace = true) {
    const container = document.getElementById("grid");

    if (replace) {
        container.innerHTML = "";
    }

    if (state.currentFiles.length === 0 && !state.loading) {
        container.innerHTML = '<div class="empty-state">No photos in this directory</div>';
        return;
    }

    let grid = container.querySelector(".photo-grid");
    if (!grid || replace) {
        grid = document.createElement("div");
        grid.className = "photo-grid";
        container.innerHTML = "";
        container.appendChild(grid);
    }

    // Only render new items (for infinite scroll)
    const startIdx = replace ? 0 : grid.children.length;

    for (let i = startIdx; i < state.currentFiles.length; i++) {
        const file = state.currentFiles[i];
        const cell = createPhotoCell(file, i);
        grid.appendChild(cell);
    }
}

function createPhotoCell(file, index) {
    const cell = document.createElement("div");
    cell.className = "photo-cell";

    const img = document.createElement("img");
    img.loading = "lazy";
    img.src = `/thumb/${file.id}`;
    img.alt = file.filename;
    // Fallback for missing thumbnails
    img.onerror = () => {
        img.style.display = "none";
        cell.style.background = "#2a2a2a";
        cell.style.display = "flex";
        cell.style.alignItems = "center";
        cell.style.justifyContent = "center";
        const text = document.createElement("span");
        text.style.color = "#555";
        text.style.fontSize = "0.7rem";
        text.textContent = file.filename;
        cell.appendChild(text);
    };

    const overlay = document.createElement("div");
    overlay.className = "overlay";

    const fname = document.createElement("span");
    fname.className = "filename";
    fname.textContent = file.filename;

    overlay.appendChild(fname);

    if (file.rating) {
        const rating = document.createElement("span");
        rating.className = "rating";
        rating.textContent = "★".repeat(file.rating);
        overlay.appendChild(rating);
    }

    cell.appendChild(img);
    cell.appendChild(overlay);

    if (file.media_type === "video") {
        const badge = document.createElement("span");
        badge.className = "video-badge";
        badge.textContent = "▶ Video";
        cell.appendChild(badge);
    }

    cell.addEventListener("click", () => openLightbox(index));

    return cell;
}

function renderFileCount() {
    const el = document.getElementById("file-count");
    if (state.totalFiles > 0) {
        const showing = state.currentFiles.length;
        el.textContent = showing < state.totalFiles
            ? `${showing} / ${state.totalFiles} files`
            : `${state.totalFiles} files`;
    } else {
        el.textContent = "";
    }
}

// ==================== Infinite Scroll ====================

function setupInfiniteScroll() {
    const grid = document.getElementById("grid");
    grid.addEventListener("scroll", () => {
        if (state.loading) return;

        const { scrollTop, scrollHeight, clientHeight } = grid;
        if (scrollHeight - scrollTop - clientHeight < 400) {
            // Near bottom — load more if there are more
            const loaded = state.currentFiles.length;
            if (loaded < state.totalFiles) {
                loadFiles(state.currentPage + 1);
            }
        }
    });
}

// ==================== Filters ====================

function renderTagChips() {
    const container = document.getElementById("tag-chips");
    container.innerHTML = "";

    // Sort by file_count descending, show top 20
    const sorted = [...state.tags].sort((a, b) => b.file_count - a.file_count).slice(0, 30);

    for (const tag of sorted) {
        const chip = document.createElement("span");
        chip.className = "tag-chip" + (state.tagFilter === tag.name ? " active" : "");
        chip.textContent = `#${tag.name}`;
        chip.title = `${tag.file_count} files`;
        chip.addEventListener("click", () => {
            if (state.tagFilter === tag.name) {
                state.tagFilter = "";
            } else {
                state.tagFilter = tag.name;
            }
            applyFilters();
        });
        container.appendChild(chip);
    }
}

function setupFilterListeners() {
    document.getElementById("rating-filter").addEventListener("change", (e) => {
        state.ratingFilter = e.target.value;
        applyFilters();
    });
}

function applyFilters() {
    state.currentPage = 1;
    state.currentFiles = [];

    if (state.ratingFilter || state.tagFilter) {
        // Use the filtered endpoint (cross-directory search)
        state.useFilteredEndpoint = true;
        // Clear directory selection visually
        state.selectedDirId = null;
        renderDirectoryTree();
    } else {
        state.useFilteredEndpoint = false;
        // If no filters and no directory, show nothing
        if (state.selectedDirId === null) {
            const grid = document.getElementById("grid");
            grid.innerHTML = '<div class="empty-state">Select a directory or apply filters</div>';
            renderFileCount();
            renderBreadcrumb();
            renderTagChips();
            return;
        }
    }

    renderBreadcrumb();
    renderTagChips();
    loadFiles(1);
}

// ==================== Lightbox ====================

function openLightbox(index) {
    state.lightboxIndex = index;
    renderLightbox();
}

function closeLightbox() {
    state.lightboxIndex = -1;
    const lb = document.getElementById("lightbox");
    lb.classList.add("hidden");
}

function renderLightbox() {
    const lb = document.getElementById("lightbox");
    const file = state.currentFiles[state.lightboxIndex];
    if (!file) {
        closeLightbox();
        return;
    }

    lb.classList.remove("hidden");

    const img = lb.querySelector("img");
    // Load the 1440px preview first, then optionally the original
    img.src = `/preview/${file.id}`;

    lb.querySelector(".filename").textContent = file.filename;
    lb.querySelector(".rating").textContent = file.rating ? "★".repeat(file.rating) : "";
    lb.querySelector(".tags").textContent = file.tags.length
        ? file.tags.map(t => "#" + t).join(" ")
        : "";
}

function lightboxPrev() {
    if (state.lightboxIndex > 0) {
        state.lightboxIndex--;
        renderLightbox();
    }
}

function lightboxNext() {
    if (state.lightboxIndex < state.currentFiles.length - 1) {
        state.lightboxIndex++;
        renderLightbox();
    }
}

function setupLightbox() {
    const lb = document.getElementById("lightbox");

    lb.querySelector(".close-btn").addEventListener("click", closeLightbox);
    lb.querySelector(".nav-prev").addEventListener("click", (e) => {
        e.stopPropagation();
        lightboxPrev();
    });
    lb.querySelector(".nav-next").addEventListener("click", (e) => {
        e.stopPropagation();
        lightboxNext();
    });

    // Click on backdrop closes
    lb.addEventListener("click", (e) => {
        if (e.target === lb) closeLightbox();
    });

    // Keyboard navigation
    document.addEventListener("keydown", (e) => {
        if (state.lightboxIndex < 0) return;

        switch (e.key) {
            case "Escape":
                closeLightbox();
                break;
            case "ArrowLeft":
                lightboxPrev();
                break;
            case "ArrowRight":
                lightboxNext();
                break;
        }
    });
}

// ==================== Init ====================

async function init() {
    try {
        // Load data in parallel
        await Promise.all([loadDirectories(), loadTags()]);

        renderDirectoryTree();
        renderTagChips();
        setupFilterListeners();
        setupInfiniteScroll();
        setupLightbox();

        // Auto-select first root if there's only one
        const roots = getRootDirectories();
        if (roots.length === 1) {
            selectDirectory(roots[0].id);
        } else {
            document.getElementById("grid").innerHTML =
                '<div class="empty-state">Select a directory to browse photos</div>';
        }
    } catch (err) {
        console.error("Failed to initialize:", err);
        document.getElementById("grid").innerHTML =
            '<div class="empty-state">Failed to connect to server</div>';
    }
}

init();
