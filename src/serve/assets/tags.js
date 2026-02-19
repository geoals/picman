// Directory rating stars, directory tag editing, and sidebar tag chips.

import { state } from './state.js';
import { setDirRating as apiSetDirRating, addDirTag as apiAddDirTag, removeDirTag as apiRemoveDirTag, loadTags } from './api.js';

// ==================== Directory Rating ====================

export function renderDirRating() {
    const container = document.getElementById("dir-rating");
    container.innerHTML = "";

    const dir = state.dirMap.get(state.selectedDirId);
    if (!dir) return;

    const currentRating = dir.rating || 0;

    for (let i = 1; i <= 5; i++) {
        const star = document.createElement("span");
        star.className = "star" + (i <= currentRating ? " filled" : "");
        star.textContent = "★";

        star.addEventListener("mouseenter", () => {
            container.querySelectorAll(".star").forEach((s, idx) => {
                s.classList.toggle("preview", idx < i && !s.classList.contains("filled"));
            });
        });

        star.addEventListener("mouseleave", () => {
            container.querySelectorAll(".star").forEach(s => s.classList.remove("preview"));
        });

        star.addEventListener("click", () => {
            const newRating = (i === currentRating) ? null : i;
            setDirRating(newRating);
        });

        container.appendChild(star);
    }
}

async function setDirRating(value) {
    const dir = state.dirMap.get(state.selectedDirId);
    if (!dir) return;

    try {
        const meta = await apiSetDirRating(dir.id, value);
        dir.rating = meta.rating;
        dir.tags = meta.tags;
        renderDirRating();
    } catch (err) {
        console.error("Failed to set rating:", err);
    }
}

// ==================== Directory Tags ====================

export function renderDirTags() {
    const container = document.getElementById("dir-tags");
    container.innerHTML = "";

    const dir = state.dirMap.get(state.selectedDirId);
    if (!dir) return;

    for (const tag of dir.tags) {
        const chip = document.createElement("span");
        chip.className = "dir-tag-chip";

        const name = document.createElement("span");
        name.textContent = "#" + tag;

        const remove = document.createElement("span");
        remove.className = "remove-tag";
        remove.classList.add("icon");
        remove.textContent = "close";
        remove.addEventListener("click", (e) => {
            e.stopPropagation();
            removeDirTag(tag);
        });

        chip.appendChild(name);
        chip.appendChild(remove);
        container.appendChild(chip);
    }

    const addBtn = document.createElement("button");
    addBtn.className = "dir-tag-add";
    addBtn.classList.add("icon");
    addBtn.textContent = "add";
    addBtn.addEventListener("click", () => showTagInput(container, addBtn));
    container.appendChild(addBtn);
}

function showTagInput(container, addBtn) {
    if (container.querySelector(".dir-tag-input-wrapper")) return;

    addBtn.style.display = "none";

    const wrapper = document.createElement("div");
    wrapper.className = "dir-tag-input-wrapper";

    const input = document.createElement("input");
    input.className = "dir-tag-input";
    input.type = "text";
    input.placeholder = "tag…";

    const suggestions = document.createElement("div");
    suggestions.className = "dir-tag-suggestions";
    suggestions.style.display = "none";

    let activeIndex = -1;

    function updateSuggestions() {
        const query = input.value.trim().toLowerCase();
        suggestions.innerHTML = "";
        activeIndex = -1;

        if (!query) {
            suggestions.style.display = "none";
            return;
        }

        const dir = state.dirMap.get(state.selectedDirId);
        const existingTags = new Set(dir ? dir.tags : []);
        const matches = state.tags
            .filter(t => t.name.includes(query) && !existingTags.has(t.name))
            .slice(0, 10);

        if (matches.length === 0) {
            suggestions.style.display = "none";
            return;
        }

        for (const tag of matches) {
            const item = document.createElement("div");
            item.className = "suggestion";
            item.textContent = "#" + tag.name;
            item.addEventListener("mousedown", (e) => {
                e.preventDefault();
                addDirTag(tag.name);
                closeInput();
            });
            suggestions.appendChild(item);
        }

        suggestions.style.display = "block";
    }

    function highlightSuggestion(index) {
        const items = suggestions.querySelectorAll(".suggestion");
        items.forEach((item, i) => item.classList.toggle("active", i === index));
        if (items[index]) items[index].scrollIntoView({ block: "nearest" });
    }

    input.addEventListener("input", updateSuggestions);

    input.addEventListener("keydown", (e) => {
        const items = suggestions.querySelectorAll(".suggestion");

        if (e.key === "ArrowDown") {
            e.preventDefault();
            activeIndex = Math.min(activeIndex + 1, items.length - 1);
            highlightSuggestion(activeIndex);
        } else if (e.key === "ArrowUp") {
            e.preventDefault();
            activeIndex = Math.max(activeIndex - 1, 0);
            highlightSuggestion(activeIndex);
        } else if (e.key === "Enter") {
            e.preventDefault();
            if (activeIndex >= 0 && items[activeIndex]) {
                const text = items[activeIndex].textContent.replace("#", "");
                addDirTag(text);
            } else if (input.value.trim()) {
                addDirTag(input.value.trim());
            }
            closeInput();
        } else if (e.key === "Escape") {
            closeInput();
        }
    });

    function closeInput() {
        wrapper.remove();
        addBtn.style.display = "";
    }

    input.addEventListener("blur", () => {
        setTimeout(closeInput, 150);
    });

    wrapper.appendChild(input);
    wrapper.appendChild(suggestions);
    container.appendChild(wrapper);
    input.focus();
}

async function addDirTag(name) {
    const dir = state.dirMap.get(state.selectedDirId);
    if (!dir) return;

    const tag = name.trim().toLowerCase();
    if (!tag) return;

    try {
        const meta = await apiAddDirTag(dir.id, tag);
        dir.rating = meta.rating;
        dir.tags = meta.tags;
        renderDirTags();
        loadTags().then(() => renderTagChips());
    } catch (err) {
        console.error("Failed to add tag:", err);
    }
}

async function removeDirTag(name) {
    const dir = state.dirMap.get(state.selectedDirId);
    if (!dir) return;

    try {
        const meta = await apiRemoveDirTag(dir.id, name);
        dir.rating = meta.rating;
        dir.tags = meta.tags;
        renderDirTags();
    } catch (err) {
        console.error("Failed to remove tag:", err);
    }
}

// ==================== Sidebar Tag Chips ====================

export function renderTagChips() {
    const container = document.getElementById("tag-chips");
    container.innerHTML = "";

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
            // Dispatch event so filters.js can handle the state change
            document.dispatchEvent(new Event("filters-changed"));
        });
        container.appendChild(chip);
    }
}
