// Duplicates view — comparison UI for resolving duplicate files.

import { state } from './state.js';
import { pushUrl, replaceUrl } from './router.js';
import { fetchDuplicatesSummary, fetchDuplicates, trashFiles, trashFolderRule } from './api.js';

// ==================== Initialization ====================

export async function initDuplicates() {
    document.getElementById('dupes-btn').addEventListener('click', () => showDuplicatesView());
    document.getElementById('dupes-back-btn').addEventListener('click', hideDuplicatesView);
    document.getElementById('dupes-type-exact').addEventListener('click', () => setType('exact'));
    document.getElementById('dupes-type-similar').addEventListener('click', () => setType('similar'));
    document.getElementById('dupes-prev').addEventListener('click', prevGroup);
    document.getElementById('dupes-skip').addEventListener('click', skipGroup);
    document.getElementById('dupes-confirm').addEventListener('click', confirmGroup);
    document.addEventListener('keydown', handleKeyboard);

    try {
        const summary = await fetchDuplicatesSummary();
        state.dupesSummary = summary;
        updateBadge(summary);
    } catch {
        // Server may not have duplicates data yet — that's fine
    }
}

function updateBadge(summary) {
    const badge = document.getElementById('dupes-badge');
    const total = summary.exact_groups + summary.similar_groups;
    if (total > 0) {
        badge.textContent = total;
        badge.classList.remove('hidden');
    } else {
        badge.classList.add('hidden');
    }
}

// ==================== View Switching ====================

export async function showDuplicatesView(type, { updateUrl = true } = {}) {
    state.view = 'duplicates';
    state.dupesCurrentGroupIndex = 0;
    state.dupesResolvedCount = 0;
    state.dupesDecisions.clear();
    state.dupesActiveFolderRule = null;

    if (type && type !== state.dupesType) {
        state.dupesType = type;
        document.getElementById('dupes-type-exact').classList.toggle('active', type === 'exact');
        document.getElementById('dupes-type-similar').classList.toggle('active', type === 'similar');
    }

    document.getElementById('library-view').classList.add('hidden');
    document.getElementById('duplicates-view').classList.remove('hidden');

    if (updateUrl) pushUrl();

    await loadGroups();
}

export function hideDuplicatesView() {
    state.view = 'library';
    document.getElementById('duplicates-view').classList.add('hidden');
    document.getElementById('library-view').classList.remove('hidden');
    pushUrl();
}

function setType(type) {
    if (state.dupesType === type) return;
    state.dupesType = type;
    state.dupesCurrentGroupIndex = 0;
    state.dupesResolvedCount = 0;
    state.dupesDecisions.clear();
    state.dupesActiveFolderRule = null;

    document.getElementById('dupes-type-exact').classList.toggle('active', type === 'exact');
    document.getElementById('dupes-type-similar').classList.toggle('active', type === 'similar');

    replaceUrl();
    loadGroups();
}

// ==================== Data Loading ====================

async function loadGroups() {
    state.dupesLoading = true;
    renderLoading();

    try {
        const data = await fetchDuplicates(
            state.dupesType, 8, state.dupesPage, state.dupesPerPage
        );
        state.dupesGroups = data.groups;
        state.dupesTotalGroups = data.total_groups;
        state.dupesFolderSuperGroups = data.folder_super_groups;

        // Pre-populate auto-suggestions for all groups
        for (const group of state.dupesGroups) {
            if (!state.dupesDecisions.has(group.group_index)) {
                applyAutoSuggestion(group);
            }
        }

        renderCurrentGroup();
    } catch (err) {
        document.getElementById('dupes-cards').innerHTML =
            `<div class="empty-state">Failed to load duplicates: ${err.message}</div>`;
    } finally {
        state.dupesLoading = false;
    }
}

// ==================== Auto-Suggestion ====================

function applyAutoSuggestion(group) {
    const decisions = new Map();
    for (const file of group.files) {
        decisions.set(file.id, file.id === group.suggested_keep_id ? 'keep' : 'trash');
    }
    state.dupesDecisions.set(group.group_index, decisions);
}

// ==================== Rendering ====================

function renderLoading() {
    document.getElementById('dupes-cards').innerHTML =
        '<div class="loading">Loading duplicates</div>';
    document.getElementById('dupes-group-header').textContent = '';
    document.getElementById('dupes-folder-rule').classList.add('hidden');
}

function renderCurrentGroup() {
    const group = getCurrentGroup();

    if (!group) {
        renderEmpty();
        return;
    }

    renderProgress();
    renderGroupHeader(group);
    renderCards(group);
    renderFolderRule(group);
    updateNavButtons();
}

function getCurrentGroup() {
    const idx = state.dupesCurrentGroupIndex;
    return state.dupesGroups[idx] || null;
}

function renderEmpty() {
    document.getElementById('dupes-group-header').textContent = '';
    document.getElementById('dupes-cards').innerHTML =
        '<div class="empty-state">No duplicates found</div>';
    document.getElementById('dupes-folder-rule').classList.add('hidden');
    document.getElementById('dupes-progress').textContent = '';
}

function renderProgress() {
    const el = document.getElementById('dupes-progress');
    const current = state.dupesCurrentGroupIndex + 1;
    const total = state.dupesGroups.length;
    const resolved = state.dupesResolvedCount;
    el.textContent = `${current}/${total} groups · ${resolved} resolved`;
}

function renderGroupHeader(group) {
    const el = document.getElementById('dupes-group-header');
    const num = state.dupesCurrentGroupIndex + 1;
    const typeLabel = group.match_type === 'exact' ? 'Exact match' :
        `Similar (distance ≤${group.max_distance})`;

    const dirs = [...new Set(group.files.map(f => f.directory_path))];
    const folderInfo = dirs.length <= 3 ? dirs.join(' · ') : `${dirs.length} folders`;

    el.innerHTML = `<span class="group-num">Group ${num}</span> — ${typeLabel}` +
        `<div class="group-folders">${folderInfo}</div>`;
}

function renderCards(group) {
    const container = document.getElementById('dupes-cards');
    const decisions = state.dupesDecisions.get(group.group_index) || new Map();

    container.innerHTML = '';

    group.files.forEach((file, index) => {
        const decision = decisions.get(file.id) || 'undecided';
        const isSuggested = file.id === group.suggested_keep_id;
        const isFocused = index === state.dupesFocusedFileIndex;

        const card = document.createElement('div');
        card.className = `dupe-card ${decision}${isFocused ? ' focused' : ''}`;
        card.dataset.fileId = file.id;
        card.dataset.index = index;

        const label = decision === 'keep' ? 'KEEP' :
            decision === 'trash' ? 'TRASH' : '';
        const labelClass = decision === 'keep' ? 'label-keep' :
            decision === 'trash' ? 'label-trash' : '';
        const suggestedHint = isSuggested && decision === 'keep' ? ' (suggested)' : '';

        const stars = file.rating ? '★'.repeat(file.rating) + '☆'.repeat(5 - file.rating) : '';
        const dims = file.width && file.height ? `${file.width}×${file.height}` : '';
        const size = formatSize(file.size);
        const tags = file.tags.length ? file.tags.map(t => `<span class="dupe-tag">#${t}</span>`).join(' ') : '';

        card.innerHTML = `
            ${label ? `<div class="dupe-label ${labelClass}">${label}${suggestedHint}</div>` : ''}
            <div class="dupe-thumb-wrap">
                <img class="dupe-thumb" src="/thumb/${file.id}" alt="${file.filename}" loading="lazy">
            </div>
            <div class="dupe-info">
                <div class="dupe-filename" title="${file.filename}">${file.filename}</div>
                <div class="dupe-dir" title="${file.directory_path}">${file.directory_path}</div>
                <div class="dupe-meta">
                    ${dims ? `<span>${dims}</span>` : ''}
                    <span>${size}</span>
                    ${file.media_type ? `<span>${file.media_type}</span>` : ''}
                </div>
                ${stars ? `<div class="dupe-rating">${stars}</div>` : ''}
                ${tags ? `<div class="dupe-tags">${tags}</div>` : ''}
            </div>
            <div class="dupe-buttons">
                <button class="btn-keep${decision === 'keep' ? ' active' : ''}"
                        data-action="keep" data-file-id="${file.id}">Keep</button>
                <button class="btn-trash${decision === 'trash' ? ' active' : ''}"
                        data-action="trash" data-file-id="${file.id}">Trash</button>
                <button class="btn-preview" data-action="preview"
                        data-file-id="${file.id}">Preview</button>
            </div>
            <div class="dupe-number">${index + 1}</div>
        `;

        card.addEventListener('click', (e) => {
            const btn = e.target.closest('[data-action]');
            if (!btn) return;

            const action = btn.dataset.action;
            const fileId = parseInt(btn.dataset.fileId);

            if (action === 'keep' || action === 'trash') {
                toggleFileDecision(group.group_index, fileId, action);
            } else if (action === 'preview') {
                previewFile(fileId);
            }
        });

        container.appendChild(card);
    });
}

function renderFolderRule(group) {
    const el = document.getElementById('dupes-folder-rule');
    const superGroup = findSuperGroup(group.group_index);

    if (!superGroup || superGroup.folders.length !== 2) {
        el.classList.add('hidden');
        return;
    }

    el.classList.remove('hidden');
    el.innerHTML = `
        <div class="folder-rule-text">
            Per-folder rule (${superGroup.group_indices.length} groups):
        </div>
        <div class="folder-rule-options">
            <button class="folder-rule-btn" data-keep-folder="0">
                Keep <strong>${superGroup.folders[0]}</strong>,
                trash ${superGroup.folders[1]}
            </button>
            <button class="folder-rule-btn" data-keep-folder="1">
                Keep <strong>${superGroup.folders[1]}</strong>,
                trash ${superGroup.folders[0]}
            </button>
        </div>
    `;

    el.querySelectorAll('.folder-rule-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const keepIdx = parseInt(btn.dataset.keepFolder);
            applyFolderRule(superGroup, keepIdx);
        });
    });

    // Show batch confirm when a folder rule is active and matches this super-group
    const rule = state.dupesActiveFolderRule;
    if (rule && superGroup.folders.includes(rule.keepFolder) && superGroup.folders.includes(rule.trashFolder)) {
        const totalCount = superGroup.group_indices.length;

        if (totalCount > 1) {
            const batchDiv = document.createElement('div');
            batchDiv.className = 'folder-rule-batch';
            batchDiv.innerHTML = `
                <button id="dupes-batch-confirm" class="batch-confirm-btn">
                    Confirm all ${totalCount} groups
                    <span class="icon">done_all</span>
                </button>
            `;
            el.appendChild(batchDiv);

            document.getElementById('dupes-batch-confirm').addEventListener('click', confirmSuperGroup);
        }
    }
}

function updateNavButtons() {
    const idx = state.dupesCurrentGroupIndex;
    document.getElementById('dupes-prev').disabled = idx === 0;
    document.getElementById('dupes-confirm').disabled = !hasAllDecisions();
}

function findSuperGroup(groupIndex) {
    return state.dupesFolderSuperGroups.find(sg =>
        sg.group_indices.includes(groupIndex)
    ) || null;
}

// ==================== Decisions ====================

function toggleFileDecision(groupIndex, fileId, action) {
    let decisions = state.dupesDecisions.get(groupIndex);
    if (!decisions) {
        decisions = new Map();
        state.dupesDecisions.set(groupIndex, decisions);
    }

    const current = decisions.get(fileId);
    // Toggle: if already this action, revert to opposite
    if (current === action) {
        decisions.set(fileId, action === 'keep' ? 'trash' : 'keep');
    } else {
        decisions.set(fileId, action);
    }

    renderCurrentGroup();
}

function toggleByIndex(fileIndex) {
    const group = getCurrentGroup();
    if (!group || fileIndex >= group.files.length) return;

    const file = group.files[fileIndex];
    const decisions = state.dupesDecisions.get(group.group_index);
    if (!decisions) return;

    const current = decisions.get(file.id);
    decisions.set(file.id, current === 'keep' ? 'trash' : 'keep');

    state.dupesFocusedFileIndex = fileIndex;
    renderCurrentGroup();
}

function acceptAutoSuggestion() {
    const group = getCurrentGroup();
    if (!group) return;
    applyAutoSuggestion(group);
    renderCurrentGroup();
}

function hasAllDecisionsForGroup(groupIndex) {
    const group = state.dupesGroups.find(g => g.group_index === groupIndex);
    if (!group) return false;
    const decisions = state.dupesDecisions.get(groupIndex);
    if (!decisions) return false;

    // Must have at least one keep and all files decided
    let hasKeep = false;
    for (const file of group.files) {
        const d = decisions.get(file.id);
        if (!d || d === 'undecided') return false;
        if (d === 'keep') hasKeep = true;
    }
    return hasKeep;
}

function hasAllDecisions() {
    const group = getCurrentGroup();
    if (!group) return false;
    return hasAllDecisionsForGroup(group.group_index);
}

// ==================== Actions ====================

async function confirmGroup() {
    const group = getCurrentGroup();
    if (!group || !hasAllDecisions()) return;

    const decisions = state.dupesDecisions.get(group.group_index);
    const toTrash = [];
    for (const file of group.files) {
        if (decisions.get(file.id) === 'trash') {
            toTrash.push(file.id);
        }
    }

    if (toTrash.length === 0) {
        // Nothing to trash, just advance
        advanceGroup();
        return;
    }

    // Disable confirm button during request
    const confirmBtn = document.getElementById('dupes-confirm');
    confirmBtn.disabled = true;
    confirmBtn.textContent = 'Trashing...';

    try {
        const result = await trashFiles(toTrash);
        state.dupesResolvedCount++;

        if (result.errors.length > 0) {
            const msgs = result.errors.map(e => `${e.file_id}: ${e.error}`).join('\n');
            console.warn('Some files failed to trash:', msgs);
        }

        // Remove trashed group from local state
        removeCurrentGroup();
    } catch (err) {
        console.error('Failed to trash files:', err);
        confirmBtn.innerHTML = 'Confirm <span class="icon">chevron_right</span>';
        confirmBtn.disabled = false;
    }
}

function skipGroup() {
    advanceGroup();
}

function advanceGroup() {
    if (state.dupesCurrentGroupIndex < state.dupesGroups.length - 1) {
        state.dupesCurrentGroupIndex++;
    } else if (state.dupesGroups.length === 0) {
        // All resolved
    }
    state.dupesFocusedFileIndex = 0;
    renderCurrentGroup();
}

function removeCurrentGroup() {
    state.dupesGroups.splice(state.dupesCurrentGroupIndex, 1);
    if (state.dupesCurrentGroupIndex >= state.dupesGroups.length) {
        state.dupesCurrentGroupIndex = Math.max(0, state.dupesGroups.length - 1);
    }
    state.dupesFocusedFileIndex = 0;
    renderCurrentGroup();
    refreshSummary();
}

async function confirmSuperGroup() {
    const rule = state.dupesActiveFolderRule;
    if (!rule) return;

    // Disable the batch confirm button during request
    const batchBtn = document.getElementById('dupes-batch-confirm');
    if (batchBtn) {
        batchBtn.disabled = true;
        batchBtn.textContent = 'Trashing...';
    }

    try {
        const result = await trashFolderRule(
            state.dupesType,
            rule.keepFolder,
            rule.trashFolder,
        );

        if (result.errors.length > 0) {
            const msgs = result.errors.map(e => `${e.file_id}: ${e.error}`).join('\n');
            console.warn('Some files failed to trash:', msgs);
        }

        state.dupesResolvedCount += result.groups_resolved;
        state.dupesActiveFolderRule = null;
        state.dupesDecisions.clear();

        // Reload groups from server to get fresh state
        await loadGroups();
        refreshSummary();
    } catch (err) {
        console.error('Failed to trash files:', err);
        if (batchBtn) {
            batchBtn.disabled = false;
            batchBtn.textContent = 'Confirm all groups';
        }
    }
}

function prevGroup() {
    if (state.dupesCurrentGroupIndex > 0) {
        state.dupesCurrentGroupIndex--;
        state.dupesFocusedFileIndex = 0;
        renderCurrentGroup();
    }
}

function applyFolderRule(superGroup, keepFolderIndex) {
    const keepFolder = superGroup.folders[keepFolderIndex];
    const trashFolder = superGroup.folders[1 - keepFolderIndex];

    // Store the active rule for server-side batch confirm
    state.dupesActiveFolderRule = { keepFolder, trashFolder };

    // Apply decisions to loaded groups for visual feedback
    for (const groupIndex of superGroup.group_indices) {
        const group = state.dupesGroups.find(g => g.group_index === groupIndex);
        if (!group) continue;

        const decisions = new Map();
        for (const file of group.files) {
            decisions.set(file.id, file.directory_path === keepFolder ? 'keep' : 'trash');
        }
        state.dupesDecisions.set(groupIndex, decisions);
    }

    renderCurrentGroup();
}

function previewFile(fileId) {
    // Open in a new tab for full preview
    window.open(`/preview/${fileId}`, '_blank');
}

async function refreshSummary() {
    try {
        const summary = await fetchDuplicatesSummary();
        state.dupesSummary = summary;
        updateBadge(summary);
    } catch {
        // Ignore
    }
}

// ==================== Keyboard Handling ====================

function handleKeyboard(e) {
    if (state.view !== 'duplicates') return;

    // Don't handle if focused on input
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;

    const group = getCurrentGroup();

    switch (e.key) {
        case 'j':
        case 'ArrowDown':
            e.preventDefault();
            if (state.dupesCurrentGroupIndex < state.dupesGroups.length - 1) {
                state.dupesCurrentGroupIndex++;
                state.dupesFocusedFileIndex = 0;
                renderCurrentGroup();
            }
            break;

        case 'k':
        case 'ArrowUp':
            e.preventDefault();
            prevGroup();
            break;

        case 'ArrowLeft':
            e.preventDefault();
            if (group && state.dupesFocusedFileIndex > 0) {
                state.dupesFocusedFileIndex--;
                renderCurrentGroup();
            }
            break;

        case 'ArrowRight':
            e.preventDefault();
            if (group && state.dupesFocusedFileIndex < group.files.length - 1) {
                state.dupesFocusedFileIndex++;
                renderCurrentGroup();
            }
            break;

        case '1': case '2': case '3': case '4': case '5':
        case '6': case '7': case '8': case '9':
            e.preventDefault();
            toggleByIndex(parseInt(e.key) - 1);
            break;

        case 'a':
            e.preventDefault();
            acceptAutoSuggestion();
            break;

        case 'Enter':
            e.preventDefault();
            if (e.shiftKey) {
                confirmSuperGroup();
            } else {
                confirmGroup();
            }
            break;

        case 's':
            e.preventDefault();
            skipGroup();
            break;

        case 'f': {
            e.preventDefault();
            if (!group) break;
            const superGroup = findSuperGroup(group.group_index);
            if (superGroup && superGroup.folders.length === 2) {
                // Toggle: keep folder[0], trash folder[1]
                applyFolderRule(superGroup, 0);
            }
            break;
        }

        case 'p':
            e.preventDefault();
            if (group && group.files[state.dupesFocusedFileIndex]) {
                previewFile(group.files[state.dupesFocusedFileIndex].id);
            }
            break;

        case 't':
            e.preventDefault();
            setType(state.dupesType === 'exact' ? 'similar' : 'exact');
            break;

        case 'Escape':
            e.preventDefault();
            hideDuplicatesView();
            break;
    }
}

// ==================== Helpers ====================

function formatSize(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
