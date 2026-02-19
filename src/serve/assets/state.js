// Application state — single source of truth shared by all modules.

export const state = {
    directories: [],
    dirMap: new Map(),
    tags: [],
    selectedDirId: null,
    expandedDirs: new Set(),
    currentFiles: [],
    totalFiles: 0,
    currentPage: 1,
    perPage: 500,
    loading: false,
    loadGeneration: 0,
    ratingFilter: "",
    tagFilter: "",
    lightboxIndex: -1,
    recursive: true,
    useFilteredEndpoint: false,
    zoomLevels: [1, 2, 3, 4, 5, 6, 8],
    zoomIndex: 3,

    // Duplicates view
    view: 'library',                // 'library' | 'duplicates'
    dupesType: 'exact',             // 'exact' | 'similar'
    dupesGroups: [],
    dupesTotalGroups: 0,
    dupesPage: 1,
    dupesPerPage: 50,
    dupesFolderSuperGroups: [],
    dupesCurrentGroupIndex: 0,
    dupesDecisions: new Map(),      // Map<groupIndex, Map<fileId, 'keep'|'trash'>>
    dupesFocusedFileIndex: 0,
    dupesResolvedCount: 0,
    dupesActiveFolderRule: null,   // { keepFolder, trashFolder } or null
    dupesLoading: false,
    dupesSummary: null,
};
