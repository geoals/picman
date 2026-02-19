// Entry point — wires modules together and initializes the app.

import { loadDirectories, loadTags } from './api.js';
import { renderDirectoryTree, selectDirectory } from './tree.js';
import { renderTagChips } from './tags.js';
import { setupFilterListeners, applyFilters } from './filters.js';
import { setupInfiniteScroll, initZoom } from './grid.js';
import { setupLightbox } from './lightbox.js';
import { initDuplicates, showDuplicatesView } from './duplicates.js';
import { initRouter } from './router.js';
import { state } from './state.js';

function navigateTo(route) {
    if (route.view === 'duplicates') {
        // If already in duplicates (e.g. type change via Back), hide library first
        showDuplicatesView(route.dupesType, { updateUrl: false });
        return;
    }

    // Switching from duplicates to library via Back button —
    // hide the DOM directly to avoid hideDuplicatesView() pushing a new URL entry.
    if (state.view === 'duplicates') {
        state.view = 'library';
        document.getElementById('duplicates-view').classList.add('hidden');
        document.getElementById('library-view').classList.remove('hidden');
    }

    // Restore filter state before navigating
    if (route.rating || route.tag) {
        state.ratingFilter = route.rating;
        state.tagFilter = route.tag;
        document.getElementById('rating-filter').value = route.rating;
        applyFilters({ updateUrl: false });
        renderTagChips();
    } else {
        selectDirectory(route.dir, {
            recursive: route.recursive,
            updateUrl: false,
        });
        renderTagChips();
    }
}

async function init() {
    try {
        await Promise.all([loadDirectories(), loadTags()]);

        renderDirectoryTree();
        renderTagChips();
        setupFilterListeners();
        setupInfiniteScroll();
        setupLightbox();
        initZoom();
        initDuplicates();

        const route = initRouter(navigateTo);
        navigateTo(route);
    } catch (err) {
        console.error("Failed to initialize:", err);
        document.getElementById("grid").innerHTML =
            '<div class="empty-state">Failed to connect to server</div>';
    }
}

init();
