// Filter controls and cross-directory search coordination.

import { state } from './state.js';
import { pushUrl } from './router.js';
import { loadFiles } from './api.js';
import { selectDirectory, renderDirectoryTree, renderBreadcrumb } from './tree.js';
import { renderGrid, renderFileCount } from './grid.js';
import { renderDirRating, renderDirTags, renderTagChips } from './tags.js';

export function setupFilterListeners() {
    document.getElementById("rating-filter").addEventListener("change", (e) => {
        state.ratingFilter = e.target.value;
        applyFilters();
    });

    // Tag chips dispatch this event when clicked
    document.addEventListener("filters-changed", () => applyFilters());
}

export async function applyFilters({ updateUrl = true } = {}) {
    state.currentPage = 1;
    state.currentFiles = [];

    if (state.ratingFilter || state.tagFilter) {
        state.useFilteredEndpoint = true;
        state.selectedDirId = null;
        renderDirectoryTree();
    } else {
        state.useFilteredEndpoint = false;
        if (state.selectedDirId === null) {
            selectDirectory("root");
            renderTagChips();
            return;
        }
    }

    if (updateUrl) pushUrl();

    renderBreadcrumb();
    renderDirRating();
    renderDirTags();
    renderTagChips();

    const loaded = await loadFiles(1);
    if (loaded) {
        renderGrid(true);
        renderFileCount();
    }
}
