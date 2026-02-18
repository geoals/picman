// Lightbox (full-screen photo viewer).

import { state } from './state.js';
import { selectDirectory } from './tree.js';

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

function goToPhotoSet() {
    const file = state.currentFiles[state.lightboxIndex];
    if (!file) return;
    closeLightbox();
    selectDirectory(file.directory_id, { recursive: false });
}

export function setupLightbox() {
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
    document.getElementById("go-to-set").addEventListener("click", (e) => {
        e.stopPropagation();
        goToPhotoSet();
    });

    lb.addEventListener("click", (e) => {
        if (e.target === lb) closeLightbox();
    });

    // Keyboard navigation (only when lightbox is open)
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

    // Listen for open requests from the photo grid
    document.addEventListener("open-lightbox", (e) => {
        openLightbox(e.detail);
    });
}
