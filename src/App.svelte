<script lang="ts">
  import {
    AlertTriangle,
    CheckCircle2,
    Eye,
    EyeOff,
    FolderOpen,
    FolderSearch,
    Plus,
    RefreshCw,
    Save,
    Search,
  } from '@lucide/svelte';
  import {
    addEnvKey,
    initialProjectPath,
    isTauriRuntime,
    loadProject,
    pickProjectDirectory,
    revealEnvValue,
    saveEnvValue
  } from './lib/tauri';
  import type { EnvEntry, EnvFile, EnvFinding, ProjectSnapshot } from './lib/types';
  import { isEntryValueHidden, keyStatus, statusLabel, totalIssueCount } from './lib/summary';

  type Notice = {
    kind: 'idle' | 'loading' | 'success' | 'error';
    message: string;
  };

  let projectPath = '.';
  let snapshot: ProjectSnapshot | null = null;
  let selectedPath = '';
  let selectedEntryId = '';
  let editValue = '';
  let filter = '';
  let filterInput: HTMLInputElement | null = null;
  let showRaw = false;
  let newEnvKey = '';
  let newEnvValue = '';
  let missingKeyDrafts: Record<string, string> = {};
  let duplicateSaveChoices: Record<string, string> = {};
  let revealedEntryRef = '';
  let revealedValue: string | null = null;
  let notice: Notice = {
    kind: 'idle',
    message: isTauriRuntime() ? 'Open a project directory to inspect env files.' : 'Browser preview is using local sample data.'
  };

  $: selectedFile = snapshot?.files.find((file) => file.path === selectedPath) ?? snapshot?.files[0] ?? null;
  $: selectedEntry = selectedFile?.entries.find((entry) => entry.id === selectedEntryId) ?? selectedFile?.entries[0] ?? null;
  $: visibleEntries = selectedFile ? filterEntries(selectedFile, filter) : [];
  $: selectedEntryRevealed = Boolean(selectedFile && selectedEntry && isEntryRevealed(selectedFile, selectedEntry));
  $: selectedEntryValue = selectedFile && selectedEntry ? entryValue(selectedFile, selectedEntry) : '';
  $: selectedValueHidden = selectedEntry ? isEntryValueHidden(selectedEntry, selectedEntryRevealed) : false;
  $: selectedEntryDuplicate = Boolean(selectedFile && selectedEntry && selectedFile.duplicateKeys.includes(selectedEntry.key));
  $: duplicateSaveAllowed =
    !selectedEntryDuplicate || Boolean(selectedFile && selectedEntry && duplicateSaveChoice(selectedFile, selectedEntry) === 'this-occurrence');
  $: normalizedNewEnvKey = newEnvKey.trim();
  $: newEnvDuplicateLines =
    selectedFile && normalizedNewEnvKey
      ? selectedFile.entries.filter((entry) => entry.key === normalizedNewEnvKey).map((entry) => entry.lineNumber)
      : [];
  $: newEnvValueType = keyInputType(normalizedNewEnvKey);
  $: selectedFileHasHiddenSecrets = Boolean(
    selectedFile?.entries.some((entry) => isEntryValueHidden(entry, isEntryRevealed(selectedFile, entry)))
  );
  $: issueCount = totalIssueCount(snapshot);
  $: canSave = Boolean(
    isTauriRuntime() && selectedFile && selectedEntry && !selectedValueHidden && duplicateSaveAllowed && editValue !== selectedEntryValue
  );
  $: canAddNewEnv = Boolean(
    isTauriRuntime() &&
      selectedFile &&
      normalizedNewEnvKey &&
      newEnvValue.trim().length > 0 &&
      newEnvDuplicateLines.length === 0 &&
      notice.kind !== 'loading'
  );
  $: findings = snapshot?.findings ?? [];
  $: missingKeyFindings = findings.filter(isAddMissingKeyFinding);
  $: advisoryFindings = findings.filter((finding) => !isAddMissingKeyFinding(finding));
  $: selectedEntryFindings =
    selectedFile && selectedEntry
      ? findings.filter((finding) => findingMatchesEntry(finding, selectedFile, selectedEntry))
      : [];
  $: attentionKeyCount = selectedFile
    ? selectedFile.entries.filter((entry) => lineAttentionClass(selectedFile, entry).length > 0).length
    : 0;
  $: selectedVisibleIndex =
    selectedFile && selectedEntry ? visibleEntries.findIndex((entry) => entry.id === selectedEntry.id) : -1;
  $: if (selectedFileHasHiddenSecrets && showRaw) {
    showRaw = false;
  }

  async function openProject(): Promise<void> {
    notice = { kind: 'loading', message: 'Scanning env files...' };
    try {
      const loaded = await loadProject(projectPath.trim() || '.');
      showRaw = false;
      clearRevealedEntry();
      const firstFile = loaded.files[0];
      const firstEntry = firstFile?.entries[0];
      applySnapshotSelection(loaded, firstFile?.path ?? '', firstEntry?.id ?? '', firstEntry?.key ?? '');
      notice = {
        kind: 'success',
        message: loaded.files.length ? `Loaded ${loaded.files.length} env file${loaded.files.length === 1 ? '' : 's'}.` : 'No env files found.'
      };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  async function bootProject(): Promise<void> {
    try {
      const initialPath = await initialProjectPath();
      if (initialPath) {
        projectPath = initialPath;
      }
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
      return;
    }

    await openProject();
  }

  function handleSubmit(event: SubmitEvent): void {
    event.preventDefault();
    void openProject();
  }

  async function browseProject(): Promise<void> {
    if (!isTauriRuntime()) {
      notice = { kind: 'error', message: 'Native folder picking is available in the Tauri desktop app.' };
      return;
    }

    try {
      const selected = await pickProjectDirectory(projectPath);
      if (!selected) {
        notice = { kind: 'idle', message: 'Folder selection cancelled.' };
        return;
      }
      projectPath = selected;
      await openProject();
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  function chooseFile(file: EnvFile): void {
    clearRevealedEntry();
    selectedPath = file.path;
    const firstEntry = file.entries[0];
    selectedEntryId = firstEntry?.id ?? '';
    editValue = firstEntry?.value ?? '';
  }

  function chooseEntry(entry: EnvEntry): void {
    const nextRef = selectedFile ? entryRevealRef(selectedFile, entry) : '';
    if (nextRef !== revealedEntryRef) {
      clearRevealedEntry();
    }
    selectedEntryId = entry.id;
    editValue = selectedFile ? entryValue(selectedFile, entry) : entry.value;
  }

  function moveSelection(delta: number): void {
    if (!selectedFile || visibleEntries.length === 0) {
      return;
    }

    const foundIndex = visibleEntries.findIndex((entry) => entry.id === selectedEntryId);
    const currentIndex = foundIndex === -1 ? (delta > 0 ? -1 : 0) : foundIndex;
    const nextIndex = Math.min(Math.max(currentIndex + delta, 0), visibleEntries.length - 1);
    chooseEntry(visibleEntries[nextIndex]);
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLButtonElement>(`.line[data-row-index="${nextIndex}"]`)?.focus();
    });
  }

  function handleKeyTableKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveSelection(1);
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveSelection(-1);
    }
  }

  function handleGlobalKeydown(event: KeyboardEvent): void {
    const target = event.target;
    const isEditing =
      target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement;

    if (event.key === '/' && !isEditing) {
      event.preventDefault();
      filterInput?.focus();
    }

    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      if (canSave) {
        void saveValue();
      }
    }

    if (event.key === 'Escape' && filter) {
      filter = '';
    }
  }

  async function saveValue(): Promise<void> {
    if (!selectedFile || !selectedEntry) {
      return;
    }

    notice = { kind: 'loading', message: `Saving ${selectedEntry.key}...` };
    try {
      const root = snapshot?.root ?? (projectPath.trim() || '.');
      const savedPath = selectedFile.path;
      const savedEntryId = selectedEntry.id;
      const savedKey = selectedEntry.key;
      const saved = await saveEnvValue(root, savedPath, savedKey, selectedEntry.lineNumber, editValue);
      applySnapshotSelection(saved, savedPath, savedEntryId, savedKey);
      notice = { kind: 'success', message: `${savedKey} saved and project diagnostics rescanned.` };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  async function addMissingKey(finding: EnvFinding): Promise<void> {
    if (!canAddMissingKey(finding) || !finding.filePath || !finding.key) {
      return;
    }

    const value = missingKeyDraft(finding).trim();
    notice = { kind: 'loading', message: `Adding ${finding.key}...` };
    try {
      const root = snapshot?.root ?? (projectPath.trim() || '.');
      const loaded = await addEnvKey(root, finding.filePath, finding.key, value);
      deleteMissingKeyDraft(finding);
      applySnapshotSelection(loaded, finding.filePath, '', finding.key);
      notice = { kind: 'success', message: `${finding.key} was added and project diagnostics rescanned.` };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  async function addNewEnv(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!selectedFile || !normalizedNewEnvKey) {
      return;
    }

    if (!isTauriRuntime()) {
      notice = { kind: 'error', message: 'Adding env keys is available in the Tauri desktop app.' };
      return;
    }

    if (newEnvDuplicateLines.length > 0) {
      notice = {
        kind: 'error',
        message: `${normalizedNewEnvKey} already exists at ${duplicateLineLabel(newEnvDuplicateLines)}.`
      };
      return;
    }

    if (!newEnvValue.trim()) {
      notice = { kind: 'error', message: `${normalizedNewEnvKey} needs a value before it can be added.` };
      return;
    }

    const targetPath = selectedFile.path;
    const key = normalizedNewEnvKey;
    const value = newEnvValue;
    notice = { kind: 'loading', message: `Adding ${key}...` };

    try {
      const loaded = await addEnvKey(currentProjectRoot(), targetPath, key, value);
      newEnvKey = '';
      newEnvValue = '';
      applySnapshotSelection(loaded, targetPath, '', key);
      notice = { kind: 'success', message: `${key} was added and project diagnostics rescanned.` };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  async function toggleSelectedReveal(): Promise<void> {
    if (!snapshot || !selectedFile || !selectedEntry || !selectedEntry.shape.redactedByDefault) {
      return;
    }

    if (selectedEntryRevealed) {
      clearRevealedEntry();
      editValue = selectedEntry.value;
      showRaw = false;
      notice = { kind: 'success', message: `${selectedEntry.key} hidden.` };
      return;
    }

    const root = currentProjectRoot();
    const filePath = selectedFile.path;
    const { key, lineNumber } = selectedEntry;
    notice = { kind: 'loading', message: `Revealing ${key}...` };

    try {
      const value = await revealEnvValue(root, filePath, key, lineNumber);
      revealedEntryRef = entryRevealRef(selectedFile, selectedEntry);
      revealedValue = value;
      editValue = value;
      notice = {
        kind: 'success',
        message: `${key} revealed until selection, hide, reload, or save.`
      };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  function inspectFinding(finding: EnvFinding): void {
    if (!finding.filePath || !finding.key || !snapshot) {
      return;
    }

    const file = snapshot.files.find((candidate) => candidate.path === finding.filePath);
    const entry =
      file?.entries.find((candidate) => candidate.id === finding.entryId) ??
      file?.entries.find((candidate) => candidate.key === finding.key && candidate.lineNumber === finding.lineNumber) ??
      file?.entries.find((candidate) => candidate.key === finding.key);
    if (!file || !entry) {
      notice = { kind: 'error', message: `Could not find ${finding.key} in the current snapshot.` };
      return;
    }

    selectedPath = file.path;
    chooseEntry(entry);
  }

  function isAddMissingKeyFinding(finding: EnvFinding): boolean {
    return finding.actionKind === 'add-missing-key' && Boolean(finding.filePath && finding.key);
  }

  function canAddMissingKey(finding: EnvFinding): boolean {
    return isTauriRuntime() && isAddMissingKeyFinding(finding) && missingKeyDraft(finding).trim().length > 0;
  }

  function canInspectFinding(finding: EnvFinding): boolean {
    return Boolean(finding.filePath && (finding.entryId || finding.key));
  }

  function currentProjectRoot(): string {
    return snapshot?.root ?? (projectPath.trim() || '.');
  }

  function missingKeyDraft(finding: EnvFinding): string {
    return missingKeyDrafts[missingKeyDraftRef(finding)] ?? '';
  }

  function setMissingKeyDraft(finding: EnvFinding, value: string): void {
    missingKeyDrafts = { ...missingKeyDrafts, [missingKeyDraftRef(finding)]: value };
  }

  function deleteMissingKeyDraft(finding: EnvFinding): void {
    const next = { ...missingKeyDrafts };
    delete next[missingKeyDraftRef(finding)];
    missingKeyDrafts = next;
  }

  function missingKeyDraftRef(finding: EnvFinding): string {
    return `${finding.filePath ?? ''}\u0000${finding.key ?? ''}`;
  }

  function missingKeyInputType(finding: EnvFinding): 'password' | 'text' {
    return keyInputType(finding.key ?? '');
  }

  function keyInputType(key: string): 'password' | 'text' {
    const upperKey = key.toUpperCase();
    const secretLike = ['SECRET', 'TOKEN', 'PASSWORD', 'API_KEY', 'PRIVATE_KEY', 'ACCESS_KEY', 'CLIENT_SECRET', 'WEBHOOK_SECRET'].some(
      (part) => upperKey.includes(part)
    );
    return secretLike ? 'password' : 'text';
  }

  function duplicateLineLabel(lines: number[]): string {
    return `${lines.length === 1 ? 'line' : 'lines'} ${lines.join(', ')}`;
  }

  function duplicateSaveChoice(file: EnvFile, entry: EnvEntry): string {
    return duplicateSaveChoices[duplicateSaveRef(file, entry)] ?? '';
  }

  function setDuplicateSaveChoice(file: EnvFile, entry: EnvEntry, value: string): void {
    duplicateSaveChoices = { ...duplicateSaveChoices, [duplicateSaveRef(file, entry)]: value };
  }

  function duplicateSaveRef(file: EnvFile, entry: EnvEntry): string {
    return `${file.path}\u0000${entry.id}`;
  }

  function applySnapshotSelection(loaded: ProjectSnapshot, preferredPath: string, preferredEntryId: string, preferredKey: string): void {
    clearRevealedEntry();
    duplicateSaveChoices = {};
    snapshot = loaded;
    const refreshedFile = loaded.files.find((file) => file.path === preferredPath) ?? loaded.files[0] ?? null;
    selectedPath = refreshedFile?.path ?? preferredPath;
    const refreshedEntry =
      refreshedFile?.entries.find((entry) => entry.id === preferredEntryId) ??
      (preferredKey ? refreshedFile?.entries.find((entry) => entry.key === preferredKey) : undefined) ??
      refreshedFile?.entries[0] ??
      null;
    selectedEntryId = refreshedEntry?.id ?? '';
    editValue = refreshedEntry?.value ?? '';
  }

  function clearRevealedEntry(): void {
    revealedEntryRef = '';
    revealedValue = null;
  }

  function entryRevealRef(file: EnvFile, entry: EnvEntry): string {
    return `${file.path}\u0000${entry.id}`;
  }

  function isEntryRevealed(file: EnvFile, entry: EnvEntry): boolean {
    return revealedValue !== null && revealedEntryRef === entryRevealRef(file, entry);
  }

  function entryValue(file: EnvFile, entry: EnvEntry): string {
    if (isEntryRevealed(file, entry)) {
      return revealedValue ?? '';
    }

    return entry.value;
  }

  function occurrenceLabel(file: EnvFile, entry: EnvEntry): string {
    const occurrences = file.entries.filter((candidate) => candidate.key === entry.key);
    const index = occurrences.findIndex((candidate) => candidate.id === entry.id);
    return `${index === -1 ? 1 : index + 1} of ${occurrences.length || 1}`;
  }

  function quoteLabel(entry: EnvEntry): string {
    if (entry.quote === '"') {
      return 'double quoted';
    }

    if (entry.quote === "'") {
      return 'single quoted';
    }

    return 'unquoted';
  }

  function diagnosticsLabel(entry: EnvEntry): string {
    return entry.diagnostics.length ? entry.diagnostics.join(', ') : 'none';
  }

  function exposureLabel(entry: EnvEntry): string {
    return entry.shape.exposure === 'browser' ? 'browser-exposed' : 'local process';
  }

  function displayValue(file: EnvFile, entry: EnvEntry): string {
    if (!isEntryValueHidden(entry, isEntryRevealed(file, entry))) {
      return entryValue(file, entry) || '(empty)';
    }

    return entry.displayValue;
  }

  function entryCategoryClass(entry: EnvEntry): string {
    if (entry.value.includes('${')) {
      return 'cat-reference';
    }

    if (entry.shape.exposure === 'browser') {
      return 'cat-public';
    }

    if (entry.shape.redactedByDefault || entry.shape.sensitive) {
      return 'cat-secret';
    }

    if (['json', 'list', 'pem'].includes(entry.shape.kind)) {
      return 'cat-structured';
    }

    return 'cat-config';
  }

  function lineAttentionClass(file: EnvFile, entry: EnvEntry): string {
    const status = keyStatus(file, entry, snapshot?.comparison ?? null);
    if (status === 'duplicate' || status === 'missing' || entry.diagnostics.length > 0) {
      return 'att-hard';
    }

    if (status === 'extra' || entry.shape.kind === 'public-secret') {
      return 'att-soft';
    }

    if (entry.shape.sensitive) {
      return 'att-info';
    }

    return '';
  }

  function lineAttentionLabel(file: EnvFile, entry: EnvEntry): string {
    const status = keyStatus(file, entry, snapshot?.comparison ?? null);
    if (status === 'duplicate') {
      return 'duplicate key';
    }

    if (status === 'missing') {
      return 'missing in actual';
    }

    if (status === 'extra') {
      return 'extra local key';
    }

    if (entry.shape.kind === 'public-secret') {
      return 'public-prefixed secret';
    }

    if (entry.diagnostics.length > 0) {
      return entry.diagnostics[0];
    }

    return entry.shape.sensitive ? 'sensitive value' : 'structured';
  }

  function expectedShapeLabel(entry: EnvEntry): string {
    if (entry.shape.kind === 'port') {
      return 'integer between 1 and 65535';
    }

    if (entry.shape.kind === 'bool') {
      return 'true or false';
    }

    if (entry.shape.kind === 'json') {
      return 'valid JSON object or array';
    }

    if (entry.shape.kind === 'credential-url') {
      return 'credential-bearing URL or DSN';
    }

    if (entry.shape.kind === 'pem') {
      return 'multiline PEM block';
    }

    if (entry.shape.kind === 'public-secret') {
      return 'public-prefixed key with secret-like name';
    }

    if (entry.shape.sensitive) {
      return `${entry.shape.label}; keep private`;
    }

    return entry.shape.reasons[0] ?? entry.shape.label;
  }

  function contractNote(file: EnvFile, entry: EnvEntry): string {
    const comparison = snapshot?.comparison;
    if (!comparison) {
      return 'No example contract loaded for this project.';
    }

    if (comparison.duplicateKeys.includes(entry.key) || file.duplicateKeys.includes(entry.key)) {
      return 'Duplicate key; save targets the selected line only after confirmation.';
    }

    if (comparison.sharedKeys.includes(entry.key)) {
      return `Present in ${comparison.examplePath}.`;
    }

    if (comparison.extraKeys.includes(entry.key)) {
      return `Local to ${file.name}; not tracked by ${comparison.examplePath}.`;
    }

    return `Optional in ${file.name}.`;
  }

  function fileRoleNote(file: EnvFile, entry: EnvEntry): string {
    const layer = snapshot?.layerReport.orderedFiles.find((candidate) => candidate.path === file.path);
    const layerText = layer ? `${layer.layerKind} layer, precedence ${layer.precedence}` : file.discoveryReasons[0] ?? 'opened directly';
    if (entry.shape.exposure === 'browser') {
      return `${file.name} exposes this key to browser bundles by name.`;
    }

    if (entry.shape.sensitive) {
      return `${file.name} is ${layerText}; keep this value server-side and redacted by default.`;
    }

    if (entry.value.includes('${')) {
      return `${file.name} references another value; vne preserves the expression instead of evaluating shell semantics.`;
    }

    return `${file.name} is ${layerText}.`;
  }

  function shapeShortLabel(entry: EnvEntry): string {
    return entry.shape.kind.replace('credential-url', 'DSN').replace('public-secret', 'public secret');
  }

  function findingMatchesEntry(finding: EnvFinding, file: EnvFile, entry: EnvEntry): boolean {
    if (finding.filePath && finding.filePath !== file.path) {
      return false;
    }

    if (finding.entryId) {
      return finding.entryId === entry.id;
    }

    if (finding.lineNumber) {
      return finding.lineNumber === entry.lineNumber && finding.key === entry.key;
    }

    return finding.key === entry.key;
  }

  function filterEntries(file: EnvFile, query: string): EnvEntry[] {
    const normalized = query.trim().toLowerCase();
    if (!normalized) {
      return file.entries;
    }

    return file.entries.filter((entry) => {
      return (
        entry.key.toLowerCase().includes(normalized) ||
        entry.shape.label.toLowerCase().includes(normalized) ||
        entry.shape.kind.toLowerCase().includes(normalized) ||
        (entry.shape.exposure?.toLowerCase().includes(normalized) ?? false) ||
        (entry.shape.sensitive && 'sensitive'.includes(normalized))
      );
    });
  }

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  void bootProject();
</script>

<svelte:head>
  <title>vne</title>
</svelte:head>

<svelte:window onkeydown={handleGlobalKeydown} />

<main class="app-shell">
  <header class="chrome">
    <div class="brandline" title={`${(snapshot?.root ?? projectPath.trim()) || '.'}/${selectedFile?.name ?? ''}`}>
      <span class="wordmark">vne</span>
      <span class="path">
        <span class="dir">{(snapshot?.root ?? projectPath.trim()) || '.'}/</span>
        <span class="file">{selectedFile?.name ?? 'no env file'}</span>
      </span>
    </div>

    <form class="project-form" onsubmit={handleSubmit}>
      <input aria-label="Project path" bind:value={projectPath} autocomplete="off" spellcheck="false" />
      <button
        class="icon-button primary"
        type="submit"
        title="Open project"
        aria-label="Open project"
        disabled={notice.kind === 'loading'}
      >
        <FolderOpen size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        class="icon-button"
        title="Browse for project directory"
        aria-label="Browse for project directory"
        disabled={notice.kind === 'loading'}
        onclick={() => void browseProject()}
      >
        <FolderSearch size={14} aria-hidden="true" />
      </button>
      <button type="button" class="icon-button" title="Reload project" aria-label="Reload project" onclick={() => void openProject()}>
        <RefreshCw size={14} aria-hidden="true" />
      </button>
    </form>

    <div class="tools">
      <label class="search-field">
        <Search size={14} aria-hidden="true" />
        <input bind:this={filterInput} bind:value={filter} placeholder="search keys" />
      </label>
    </div>
  </header>

  <div class="semantic-main">
    <section class="editor" aria-label="Environment buffer">
      <div class="file-strip" aria-label="Environment files">
        {#if snapshot && snapshot.files.length > 0}
          {#each snapshot.files as file}
            <button class:active={file.path === selectedPath} type="button" onclick={() => chooseFile(file)}>
              <span>{file.name}</span>
            </button>
          {/each}
          {#if selectedFile}
            <span class="file-meta">{selectedFile.entries.length} keys / {selectedFile.duplicateKeys.length} dup</span>
          {/if}
        {:else}
          <span>No env-like files found</span>
        {/if}
      </div>

      {#if selectedFile}
        <form class="add-env-bar" aria-label={`Add env key to ${selectedFile.name}`} onsubmit={addNewEnv}>
          <input
            class="new-env-key"
            aria-label="New env key"
            bind:value={newEnvKey}
            placeholder="NEW_KEY"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
          />
          <input
            class="new-env-value"
            aria-label="New env value"
            bind:value={newEnvValue}
            type={newEnvValueType}
            placeholder="value"
            autocomplete="off"
            spellcheck="false"
          />
          <button class="peek-action primary add-env-button" type="submit" disabled={!canAddNewEnv}>
            <Plus size={14} aria-hidden="true" />
            <span>add env</span>
          </button>
          {#if newEnvDuplicateLines.length > 0}
            <span class="duplicate-note">duplicate at {duplicateLineLabel(newEnvDuplicateLines)}</span>
          {/if}
        </form>
      {/if}

      <div class="buffer" role="table" aria-label="Parsed env lines">
        {#if selectedFile && visibleEntries.length > 0}
          {#each visibleEntries as entry, rowIndex}
            {@const status = keyStatus(selectedFile, entry, snapshot?.comparison ?? null)}
            {@const att = lineAttentionClass(selectedFile, entry)}
            <div class={`row-wrap ${entryCategoryClass(entry)}`} class:open={entry.id === selectedEntryId}>
              <button
                class="line"
                class:sel={entry.id === selectedEntryId}
                class:ghost={status === 'missing'}
                data-row-index={rowIndex}
                type="button"
                role="row"
                onclick={() => chooseEntry(entry)}
                onkeydown={handleKeyTableKeydown}
              >
                <span class="marker" role="cell"><span class={`pip ${att}`}></span></span>
                <span class="gutter" role="cell">{entry.lineNumber}</span>
                <span class="code" role="cell">
                  <span class="k">{entry.key}</span><span class="eq">=</span><span
                    class:redacted={isEntryValueHidden(entry, isEntryRevealed(selectedFile, entry))}
                    class="v"
                  >{displayValue(selectedFile, entry)}</span>{#if entry.comment}<span class="comment"> {entry.comment}</span>{/if}
                </span>
                <span class="annot" role="cell">
                  <span class="typ">{shapeShortLabel(entry)}</span>
                  <span class="exp">{exposureLabel(entry)}</span>
                  {#if att}<span class="attention">{lineAttentionLabel(selectedFile, entry)}</span>{/if}
                </span>
              </button>

              {#if entry.id === selectedEntryId}
                <section class="line-edit" aria-label={`Edit ${entry.key}`}>
                  <div class="line-edit-head">
                    <strong>{entry.key}</strong>
                    <span>{entry.shape.label} / {entry.shape.confidence} / {statusLabel(status)}</span>
                  </div>

                  {#if selectedValueHidden}
                    <textarea class="line-editor" value={entry.displayValue} spellcheck="false" rows="2" readonly></textarea>
                  {:else}
                    <textarea class="line-editor" bind:value={editValue} spellcheck="false" rows="2"></textarea>
                  {/if}

                  {#if selectedEntryDuplicate}
                    <label class="inline-select" for="peek-duplicate-save-target">
                      <span>duplicate target</span>
                      <select
                        id="peek-duplicate-save-target"
                        value={duplicateSaveChoice(selectedFile, entry)}
                        onchange={(event) => setDuplicateSaveChoice(selectedFile, entry, event.currentTarget.value)}
                      >
                        <option value="">Choose target</option>
                        <option value="this-occurrence">This occurrence only ({occurrenceLabel(selectedFile, entry)})</option>
                      </select>
                    </label>
                  {/if}

                  <div class="line-edit-actions">
                    <button
                      type="button"
                      class="peek-action"
                      disabled={notice.kind === 'loading' || !entry.shape.redactedByDefault}
                      onclick={() => void toggleSelectedReveal()}
                    >
                      {#if selectedEntryRevealed}
                        <EyeOff size={14} aria-hidden="true" />
                        <span>hide value</span>
                      {:else}
                        <Eye size={14} aria-hidden="true" />
                        <span>reveal value</span>
                      {/if}
                    </button>
                    <button class="peek-action primary" type="button" disabled={!canSave} onclick={() => void saveValue()}>
                      <Save size={14} aria-hidden="true" />
                      <span>save</span>
                    </button>
                    <button type="button" class="peek-action" disabled={selectedFileHasHiddenSecrets} onclick={() => (showRaw = !showRaw)}>
                      <span>{showRaw ? 'hide raw' : 'raw'}</span>
                    </button>
                    <span class="line-hint">{selectedEntryFindings[0]?.title ?? expectedShapeLabel(entry)} / {contractNote(selectedFile, entry)}</span>
                  </div>

                  {#if showRaw}
                    <pre>{selectedFile.content}</pre>
                  {/if}
                </section>
              {/if}
            </div>
          {/each}
        {:else}
          <p class="empty-copy">No keys match this view.</p>
        {/if}
      </div>
    </section>
  </div>

  <footer class="status-strip" class:error={notice.kind === 'error'} class:success={notice.kind === 'success'}>
    {#if notice.kind === 'error'}
      <AlertTriangle size={14} aria-hidden="true" />
    {:else}
      <CheckCircle2 size={14} aria-hidden="true" />
    {/if}
    <span>{notice.message}</span>
    <span class="sep"></span>
    <span class:quiet={attentionKeyCount === 0} class="attention-summary">
      <span class="mini-dot"></span>
      {attentionKeyCount === 0 ? 'nothing needs attention' : `${attentionKeyCount} keys to understand`}
    </span>
    {#if snapshot?.comparison}
      <span class="sep"></span>
      <span>
        {snapshot.comparison.sharedKeys.length}/{snapshot.comparison.sharedKeys.length + snapshot.comparison.missingKeys.length} contract /
        {snapshot.comparison.missingKeys.length} missing / {snapshot.comparison.extraKeys.length} extra
      </span>
    {/if}
    <span class="spacer"></span>
    <button type="button" onclick={() => moveSelection(-1)} disabled={selectedVisibleIndex <= 0}>prev</button>
    <button type="button" onclick={() => moveSelection(1)} disabled={selectedVisibleIndex === -1 || selectedVisibleIndex >= visibleEntries.length - 1}>
      next
    </button>
    <span>{selectedEntry ? `ln ${selectedEntry.lineNumber} / ${selectedEntry.key}` : 'no key'}</span>
  </footer>
</main>
