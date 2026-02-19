// URL-based state persistence — maps browser URL query params to app state.
// Leaf module: only imports state.js, no circular dependencies.

import { state } from './state.js';

function stateToParams() {
    const params = new URLSearchParams();

    if (state.view === 'duplicates') {
        params.set('view', 'dupes');
        if (state.dupesType !== 'exact') params.set('type', state.dupesType);
        return params;
    }

    // Library view
    if (state.selectedDirId && state.selectedDirId !== 'root') {
        params.set('dir', state.selectedDirId);
        if (!state.recursive) params.set('recursive', 'false');
    }
    if (state.ratingFilter) params.set('rating', state.ratingFilter);
    if (state.tagFilter) params.set('tag', state.tagFilter);

    return params;
}

function buildUrl(params) {
    const qs = params.toString();
    return qs ? `/?${qs}` : '/';
}

export function pushUrl() {
    const url = buildUrl(stateToParams());
    if (url !== window.location.pathname + window.location.search) {
        history.pushState(null, '', url);
    }
}

export function replaceUrl() {
    const url = buildUrl(stateToParams());
    history.replaceState(null, '', url);
}

export function readUrl() {
    const params = new URLSearchParams(window.location.search);

    if (params.get('view') === 'dupes') {
        return {
            view: 'duplicates',
            dupesType: params.get('type') || 'exact',
        };
    }

    const dirRaw = params.get('dir');
    return {
        view: 'library',
        dir: dirRaw ? parseInt(dirRaw, 10) : 'root',
        recursive: params.get('recursive') !== 'false',
        rating: params.get('rating') || '',
        tag: params.get('tag') || '',
    };
}

export function initRouter(onNavigate) {
    window.addEventListener('popstate', () => onNavigate(readUrl()));
    return readUrl();
}
