// Entry point — wires modules together and initializes the app.

import { loadDirectories, loadTags } from './api.js';
import { renderDirectoryTree, selectDirectory } from './tree.js';
import { renderTagChips } from './tags.js';
import { setupFilterListeners } from './filters.js';
import { setupInfiniteScroll, initZoom } from './grid.js';
import { setupLightbox } from './lightbox.js';
import { initDuplicates } from './duplicates.js';

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

        selectDirectory("root");
    } catch (err) {
        console.error("Failed to initialize:", err);
        document.getElementById("grid").innerHTML =
            '<div class="empty-state">Failed to connect to server</div>';
    }
}

init();
