// Photo grid rendering, infinite scroll, and zoom controls.

import { state } from './state.js';
import { loadFiles } from './api.js';

// Track estimated column heights for shortest-column-first placement.
let columnHeights = [];

export function renderGrid(replace = true) {
    const container = document.getElementById("grid");
    const columnCount = state.zoomLevels[state.zoomIndex];

    if (state.currentFiles.length === 0 && !state.loading) {
        container.innerHTML = '<div class="empty-state">No photos in this directory</div>';
        return;
    }

    let grid = container.querySelector(".photo-grid");

    let existingCount = 0;
    if (grid && !replace) {
        for (const col of grid.children) {
            existingCount += col.children.length;
        }
    }

    if (!grid || replace || grid.children.length !== columnCount) {
        existingCount = 0;
        columnHeights = new Array(columnCount).fill(0);
        grid = document.createElement("div");
        grid.className = "photo-grid";
        container.innerHTML = "";
        container.appendChild(grid);

        for (let c = 0; c < columnCount; c++) {
            const col = document.createElement("div");
            col.className = "photo-column";
            grid.appendChild(col);
        }
    }

    for (let i = existingCount; i < state.currentFiles.length; i++) {
        let minCol = 0;
        for (let c = 1; c < columnCount; c++) {
            if (columnHeights[c] < columnHeights[minCol]) minCol = c;
        }

        const file = state.currentFiles[i];
        const ratio = (file.width && file.height) ? file.width / file.height : 3 / 2;
        columnHeights[minCol] += 1 / ratio;

        grid.children[minCol].appendChild(createPhotoCell(file, i));
    }
}

function createPhotoCell(file, index) {
    const cell = document.createElement("div");
    cell.className = "photo-cell";

    const img = document.createElement("img");
    img.loading = "lazy";
    // Always set aspect-ratio so unloaded images reserve the correct space.
    // Must match the ratio used in renderGrid's height tracking.
    if (file.width && file.height) {
        img.style.aspectRatio = `${file.width} / ${file.height}`;
    } else {
        img.style.aspectRatio = '3 / 2';
    }
    img.src = `/thumb/${file.id}`;
    img.alt = file.filename;
    img.onerror = () => {
        img.style.display = "none";
        cell.style.background = "#2a2a2a";
        cell.style.height = "120px";
        cell.style.display = "flex";
        cell.style.alignItems = "center";
        cell.style.justifyContent = "center";
        cell.style.lineHeight = "normal";
        const text = document.createElement("span");
        text.style.color = "#555";
        text.style.fontSize = "0.7rem";
        text.style.padding = "8px";
        text.style.wordBreak = "break-all";
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

    // Dispatch event instead of importing lightbox directly to avoid a dependency cycle
    // (grid → lightbox → tree → grid). lightbox.js listens for this event.
    cell.addEventListener("click", () => {
        document.dispatchEvent(new CustomEvent("open-lightbox", { detail: index }));
    });

    return cell;
}

export function renderFileCount() {
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

export function setupInfiniteScroll() {
    const grid = document.getElementById("grid");
    grid.addEventListener("scroll", async () => {
        if (state.loading) return;

        const { scrollTop, scrollHeight, clientHeight } = grid;
        if (scrollHeight - scrollTop - clientHeight < 400) {
            const loaded = state.currentFiles.length;
            if (loaded < state.totalFiles) {
                const ok = await loadFiles(state.currentPage + 1);
                if (ok) {
                    renderGrid(false);
                    renderFileCount();
                }
            }
        }
    });
}

export function initZoom() {
    const saved = localStorage.getItem("picman-zoom-index");
    if (saved !== null) {
        const idx = parseInt(saved, 10);
        if (idx >= 0 && idx < state.zoomLevels.length) {
            state.zoomIndex = idx;
        }
    }

    document.getElementById("zoom-in").addEventListener("click", zoomIn);
    document.getElementById("zoom-out").addEventListener("click", zoomOut);

    document.getElementById("grid").addEventListener("wheel", (e) => {
        if (!e.ctrlKey) return;
        e.preventDefault();
        if (e.deltaY > 0) zoomOut();
        else if (e.deltaY < 0) zoomIn();
    }, { passive: false });

    // Keyboard zoom (only when lightbox is closed)
    document.addEventListener("keydown", (e) => {
        if (state.lightboxIndex >= 0) return;
        if (e.key === "+" || e.key === "=") zoomOut();
        else if (e.key === "-") zoomIn();
    });
}

function applyZoom() {
    if (document.querySelector(".photo-grid")) {
        renderGrid(true);
    }
    document.getElementById("zoom-in").disabled = state.zoomIndex <= 0;
    document.getElementById("zoom-out").disabled = state.zoomIndex >= state.zoomLevels.length - 1;
}

function zoomIn() {
    if (state.zoomIndex <= 0) return;
    state.zoomIndex--;
    localStorage.setItem("picman-zoom-index", state.zoomIndex);
    applyZoom();
}

function zoomOut() {
    if (state.zoomIndex >= state.zoomLevels.length - 1) return;
    state.zoomIndex++;
    localStorage.setItem("picman-zoom-index", state.zoomIndex);
    applyZoom();
}
