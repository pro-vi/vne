<script lang="ts">
  import {
    AlertTriangle,
    CheckCircle2,
    Eye,
    EyeOff,
    FileKey2,
    FolderOpen,
    LoaderCircle,
    Plus,
    RefreshCw,
    Save,
    Search,
    X,
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
  import type { EnvComparison, EnvEntry, EnvFile, EnvFinding, KeyStatus, ProjectSnapshot } from './lib/types';
  import { isEntryValueHidden, keyStatus, statusLabel } from './lib/summary';

  type Notice = {
    kind: 'idle' | 'loading' | 'success' | 'error';
    message: string;
  };

  type EntryRow = {
    entry: EnvEntry;
    status: KeyStatus;
    attentionClass: string;
    attentionLabel: string;
    categoryClass: string;
    shapeLabel: string;
    displayValue: string;
    valueHidden: boolean;
  };

  let projectPath = '.';
  let snapshot: ProjectSnapshot | null = null;
  let selectedPath = '';
  let selectedEntryId = '';
  let editValue = '';
  let filter = '';
  let filterInput: HTMLInputElement | null = null;
  let reviewToggleButton: HTMLButtonElement | null = null;
  let reviewCloseButton: HTMLButtonElement | null = null;
  let viewportWidth = 1180;
  let showRaw = false;
  let showAddKey = false;
  let showReview = false;
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
  $: selectedEntry = selectedFile?.entries.find((entry) => entry.id === selectedEntryId) ?? null;
  $: currentComparison = snapshot?.comparison ?? null;
  $: visibleEntries = selectedFile ? filterEntries(selectedFile, filter) : [];
  $: entryRows = selectedFile ? buildEntryRows(selectedFile, visibleEntries, currentComparison) : [];
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
  $: hasDirtyEdit = Boolean(selectedEntry && !selectedValueHidden && editValue !== selectedEntryValue);
  $: hasNewKeyDraft = Boolean(newEnvKey || newEnvValue);
  $: hasMissingKeyDrafts = Object.values(missingKeyDrafts).some((value) => value.length > 0);
  $: canSave = Boolean(
    isTauriRuntime() &&
      notice.kind !== 'loading' &&
      selectedFile &&
      selectedEntry &&
      !selectedValueHidden &&
      duplicateSaveAllowed &&
      editValue !== selectedEntryValue
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
  $: attentionKeyCount = selectedFile ? countAttentionKeys(selectedFile, currentComparison) : 0;
  $: selectedVisibleIndex =
    selectedFile && selectedEntry ? entryRows.findIndex((row) => row.entry.id === selectedEntry.id) : -1;
  $: projectRoot = snapshot?.root ?? (projectPath.trim() || '.');
  $: projectLabel = projectName(projectRoot);
  $: reviewIsModal = viewportWidth <= 980;
  $: if (selectedFileHasHiddenSecrets && showRaw) {
    showRaw = false;
  }

  async function openProject(confirmDiscard = true): Promise<void> {
    if (confirmDiscard && !confirmDiscardedWork('project')) {
      return;
    }

    const requestedPath = projectPath.trim() || '.';
    notice = { kind: 'loading', message: 'Scanning env files...' };
    try {
      const loaded = await loadProject(requestedPath);
      showRaw = false;
      showAddKey = false;
      showReview = false;
      filter = '';
      newEnvKey = '';
      newEnvValue = '';
      missingKeyDrafts = {};
      clearRevealedEntry();
      const firstFile = loaded.files[0];
      applySnapshotSelection(loaded, firstFile?.path ?? '', '', '');
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

    await openProject(false);
  }

  async function browseProject(): Promise<void> {
    if (!confirmDiscardedWork('project')) {
      return;
    }

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
      await openProject(false);
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  function chooseFile(file: EnvFile): void {
    if (file.path === selectedPath || !confirmDiscardedWork('file')) {
      return;
    }

    clearRevealedEntry();
    showRaw = false;
    showAddKey = false;
    newEnvKey = '';
    newEnvValue = '';
    selectedPath = file.path;
    selectedEntryId = '';
    editValue = '';
  }

  function chooseEntry(entry: EnvEntry, confirmDiscard = true): boolean {
    if (entry.id === selectedEntryId) {
      return true;
    }

    if (confirmDiscard && !confirmDiscardedWork('entry')) {
      return false;
    }

    const nextRef = selectedFile ? entryRevealRef(selectedFile, entry) : '';
    if (nextRef !== revealedEntryRef) {
      clearRevealedEntry();
    }
    selectedEntryId = entry.id;
    editValue = selectedFile ? entryValue(selectedFile, entry) : entry.value;
    return true;
  }

  function moveSelection(delta: number): void {
    if (!selectedFile || visibleEntries.length === 0) {
      return;
    }

    const foundIndex = visibleEntries.findIndex((entry) => entry.id === selectedEntryId);
    const currentIndex = foundIndex === -1 ? (delta > 0 ? -1 : 0) : foundIndex;
    const nextIndex = Math.min(Math.max(currentIndex + delta, 0), visibleEntries.length - 1);
    if (!chooseEntry(visibleEntries[nextIndex])) {
      return;
    }
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

    if (event.key === 'Escape' && showReview) {
      event.preventDefault();
      closeReview();
      return;
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

    const changesFile = finding.filePath !== selectedPath;
    if (!confirmDiscardedWork(changesFile ? 'file' : 'entry')) {
      return;
    }

    if (changesFile) {
      showAddKey = false;
      newEnvKey = '';
      newEnvValue = '';
    }

    const value = missingKeyDraft(finding).trim();
    notice = { kind: 'loading', message: `Adding ${finding.key}...` };
    try {
      const root = snapshot?.root ?? (projectPath.trim() || '.');
      const loaded = await addEnvKey(root, finding.filePath, finding.key, value);
      deleteMissingKeyDraft(finding);
      filter = '';
      applySnapshotSelection(loaded, finding.filePath, '', finding.key);
      closeReview(false);
      window.requestAnimationFrame(() => document.querySelector<HTMLButtonElement>('.line.sel')?.focus());
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

    if (!confirmDiscardedWork('entry')) {
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
      filter = '';
      applySnapshotSelection(loaded, targetPath, '', key);
      showAddKey = false;
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
      if (!confirmDiscardedWork('entry')) {
        return;
      }
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

  function inspectFinding(finding: EnvFinding): boolean {
    if (!finding.key || !snapshot) {
      return false;
    }

    const file = finding.filePath
      ? snapshot.files.find((candidate) => candidate.path === finding.filePath)
      : snapshot.files.find((candidate) => candidate.entries.some((entry) => entry.key === finding.key));
    const entry =
      file?.entries.find((candidate) => candidate.id === finding.entryId) ??
      file?.entries.find((candidate) => candidate.key === finding.key && candidate.lineNumber === finding.lineNumber) ??
      file?.entries.find((candidate) => candidate.key === finding.key);
    if (!file || !entry) {
      notice = { kind: 'error', message: `Could not find ${finding.key} in the current snapshot.` };
      return false;
    }

    const changesFile = file.path !== selectedPath;
    if (!confirmDiscardedWork(changesFile ? 'file' : 'entry')) {
      return false;
    }

    if (changesFile) {
      showAddKey = false;
      newEnvKey = '';
      newEnvValue = '';
    }

    filter = '';
    selectedPath = file.path;
    chooseEntry(entry, false);
    return true;
  }

  function openFinding(finding: EnvFinding): void {
    if (inspectFinding(finding)) {
      closeReview(false);
      window.requestAnimationFrame(() => {
        document.querySelector<HTMLButtonElement>('.line.sel')?.focus({ preventScroll: true });
        document.querySelector<HTMLElement>('.line-edit')?.scrollIntoView({ block: 'nearest' });
      });
    }
  }

  function toggleReview(): void {
    if (showReview) {
      closeReview();
      return;
    }

    showReview = true;
    window.requestAnimationFrame(() => reviewCloseButton?.focus());
  }

  function closeReview(restoreFocus = true): void {
    showReview = false;
    if (restoreFocus) {
      window.requestAnimationFrame(() => reviewToggleButton?.focus());
    }
  }

  function isAddMissingKeyFinding(finding: EnvFinding): boolean {
    return finding.actionKind === 'add-missing-key' && Boolean(finding.filePath && finding.key);
  }

  function canAddMissingKey(finding: EnvFinding): boolean {
    return (
      isTauriRuntime() &&
      notice.kind !== 'loading' &&
      isAddMissingKeyFinding(finding) &&
      missingKeyDraft(finding).trim().length > 0
    );
  }

  function canInspectFinding(finding: EnvFinding): boolean {
    return Boolean(finding.key && snapshot?.files.some((file) => file.entries.some((entry) => entry.key === finding.key)));
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
    projectPath = loaded.root;
    const refreshedFile = loaded.files.find((file) => file.path === preferredPath) ?? loaded.files[0] ?? null;
    selectedPath = refreshedFile?.path ?? preferredPath;
    const refreshedEntry =
      (preferredEntryId ? refreshedFile?.entries.find((entry) => entry.id === preferredEntryId) : undefined) ??
      (preferredKey ? refreshedFile?.entries.find((entry) => entry.key === preferredKey) : undefined) ??
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

  function exposureLabel(entry: EnvEntry): string {
    return entry.shape.exposure === 'browser' ? 'browser-exposed' : 'local process';
  }

  function buildEntryRows(file: EnvFile, entries: EnvEntry[], comparison: EnvComparison | null): EntryRow[] {
    return entries.map((entry) => {
      const status = keyStatus(file, entry, comparison);
      const attentionClass = entryAttentionClass(entry, status);
      const valueHidden = isEntryValueHidden(entry, isEntryRevealed(file, entry));

      return {
        entry,
        status,
        attentionClass,
        attentionLabel: attentionClass ? entryAttentionLabel(entry, status) : '',
        categoryClass: entryCategoryClass(entry),
        shapeLabel: shapeShortLabel(entry),
        displayValue: valueHidden ? entry.displayValue : entryValue(file, entry) || '(empty)',
        valueHidden
      };
    });
  }

  function countAttentionKeys(file: EnvFile, comparison: EnvComparison | null): number {
    return file.entries.filter((entry) => entryAttentionClass(entry, keyStatus(file, entry, comparison)).length > 0).length;
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

  function entryAttentionClass(entry: EnvEntry, status: KeyStatus): string {
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

  function entryAttentionLabel(entry: EnvEntry, status: KeyStatus): string {
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

  function confirmDiscardedWork(scope: 'entry' | 'file' | 'project'): boolean {
    const hasPendingWork =
      hasDirtyEdit || (scope !== 'entry' && hasNewKeyDraft) || (scope === 'project' && hasMissingKeyDrafts);
    return !hasPendingWork || window.confirm('Discard your unsaved env changes?');
  }

  function projectName(path: string): string {
    const normalized = path.replace(/[\\/]+$/, '');
    return normalized.split(/[\\/]/).pop() || path;
  }

  void bootProject();
</script>

<svelte:head>
  <title>vne</title>
</svelte:head>

<svelte:window bind:innerWidth={viewportWidth} onkeydown={handleGlobalKeydown} />

<main class="app-shell">
  <header class="chrome" inert={showReview && reviewIsModal}>
    <div class="project-lockup" title={projectRoot}>
      <span class="app-mark" title="vne"><FileKey2 size={16} aria-hidden="true" /></span>
      <span class="project-identity">
        <strong>{projectLabel}</strong>
        <span>{projectRoot}</span>
      </span>
    </div>

    <div class="project-actions">
      <button
        type="button"
        class="chrome-action"
        title={isTauriRuntime() ? 'Choose another project folder' : 'Folder picking requires the desktop app'}
        disabled={notice.kind === 'loading' || !isTauriRuntime()}
        onclick={() => void browseProject()}
      >
        <FolderOpen size={15} aria-hidden="true" />
        <span>Change folder</span>
      </button>
      <button
        type="button"
        class="chrome-action icon-only"
        title="Reload project"
        aria-label="Reload project"
        disabled={notice.kind === 'loading'}
        onclick={() => void openProject()}
      >
        <RefreshCw size={15} aria-hidden="true" />
      </button>
    </div>
  </header>

  <div
    class:with-review={showReview && findings.length > 0}
    class:review-overlay={showReview && findings.length > 0 && reviewIsModal}
    class="semantic-main"
  >
    <section class="editor" aria-label="Environment workspace" inert={showReview && reviewIsModal}>
      {#if snapshot && snapshot.files.length > 0}
        <div class="workspace-bar">
          <nav class="file-tabs" aria-label="Environment files">
            {#each snapshot.files as file}
              <button
                class:active={file.path === selectedPath}
                type="button"
                aria-pressed={file.path === selectedPath}
                title={file.path}
                onclick={() => chooseFile(file)}
              >
                <span>{file.name}</span>
                <span class="tab-count">{file.entries.length}</span>
                {#if file.duplicateKeys.length > 0}<span class="tab-alert" aria-label={`${file.duplicateKeys.length} duplicate keys`}></span>{/if}
              </button>
            {/each}
          </nav>

          <label class="search-field">
            <Search size={14} aria-hidden="true" />
            <span class="sr-only">Filter environment keys</span>
            <input
              bind:this={filterInput}
              bind:value={filter}
              aria-label="Filter environment keys"
              placeholder="Filter keys  /"
              autocomplete="off"
              spellcheck="false"
            />
            {#if filter}
              <button type="button" aria-label="Clear key filter" title="Clear filter" onclick={() => (filter = '')}>
                <X size={13} aria-hidden="true" />
              </button>
            {/if}
          </label>

          <button
            type="button"
            class:on={showAddKey}
            class="toolbar-action"
            aria-expanded={showAddKey}
            onclick={() => (showAddKey = !showAddKey)}
          >
            <Plus size={14} aria-hidden="true" />
            <span>Add key</span>
          </button>
        </div>

        {#if notice.kind === 'error'}
          <div class="inline-notice error" role="alert">
            <AlertTriangle size={15} aria-hidden="true" />
            <span>{notice.message} The loaded project is still shown below.</span>
            <button type="button" onclick={() => void openProject()}>Try again</button>
          </div>
        {/if}

        <section class:quiet={findings.length === 0} class="attention-band" aria-label="Project review summary">
          <span class="summary-icon">
            {#if findings.length > 0}
              <AlertTriangle size={16} aria-hidden="true" />
            {:else}
              <CheckCircle2 size={16} aria-hidden="true" />
            {/if}
          </span>
          <span class="summary-copy">
            <strong>{findings.length === 0 ? 'Project checks are clear' : `${findings.length} finding${findings.length === 1 ? '' : 's'} need review`}</strong>
            <span>
              {#if findings.length === 0}
                No missing, duplicate, or layered env issues detected.
              {:else}
                {missingKeyFindings.length} missing · {advisoryFindings.length} advisory · {snapshot.layerReport.overrides.length} layered override{snapshot.layerReport.overrides.length === 1 ? '' : 's'}
              {/if}
            </span>
          </span>
          {#if findings.length > 0}
            <button
              bind:this={reviewToggleButton}
              type="button"
              class:on={showReview}
              class="review-toggle"
              aria-expanded={showReview}
              onclick={toggleReview}
            >
              {showReview ? 'Hide review' : 'Review findings'}
            </button>
          {/if}
        </section>

        {#if showAddKey && selectedFile}
          <form class="add-env-bar" aria-label={`Add env key to ${selectedFile.name}`} onsubmit={addNewEnv}>
            <span class="add-env-target">Add to <strong>{selectedFile.name}</strong></span>
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
            <button class="compact-action primary" type="submit" disabled={!canAddNewEnv}>
              <Plus size={14} aria-hidden="true" />
              <span>Add</span>
            </button>
            <button type="button" class="compact-action" aria-label="Close add key form" onclick={() => (showAddKey = false)}>
              <X size={14} aria-hidden="true" />
            </button>
            {#if newEnvDuplicateLines.length > 0}
              <span class="duplicate-note">{normalizedNewEnvKey} already exists at {duplicateLineLabel(newEnvDuplicateLines)}.</span>
            {/if}
          </form>
        {/if}

        <div class="buffer" role="list" aria-label={`Parsed keys in ${selectedFile?.name ?? 'environment file'}`}>
          {#if selectedFile && visibleEntries.length > 0}
            {#each entryRows as row, rowIndex}
              {@const entry = row.entry}
              <div class={`row-wrap ${row.categoryClass}`} class:open={entry.id === selectedEntryId} role="listitem">
                <button
                  class="line"
                  class:sel={entry.id === selectedEntryId}
                  class:ghost={row.status === 'missing'}
                  data-row-index={rowIndex}
                  type="button"
                  aria-expanded={entry.id === selectedEntryId}
                  aria-label={`${entry.key}, line ${entry.lineNumber}, ${row.shapeLabel}, ${row.attentionLabel || statusLabel(row.status)}`}
                  onclick={() => chooseEntry(entry)}
                  onkeydown={handleKeyTableKeydown}
                >
                  <span class="marker"><span class={`pip ${row.attentionClass}`}></span></span>
                  <span class="gutter">{entry.lineNumber}</span>
                  <span class="code">
                    <span class="k">{entry.key}</span><span class="eq">=</span><span class:redacted={row.valueHidden} class="v">{row.displayValue}</span>{#if entry.comment}<span class="comment"> {entry.comment}</span>{/if}
                  </span>
                  <span class="annot">
                    <span class="typ">{row.shapeLabel}</span>
                    <span class="exp">{exposureLabel(entry)}</span>
                    {#if row.attentionClass}<span class="attention">{row.attentionLabel}</span>{/if}
                  </span>
                </button>

                {#if entry.id === selectedEntryId}
                  <section class="line-edit" aria-label={`Edit ${entry.key}`}>
                    <div class="line-edit-head">
                      <strong>{entry.key}</strong>
                      <span>{entry.shape.label} · {entry.shape.confidence} confidence · {statusLabel(row.status)}</span>
                      {#if hasDirtyEdit}<span class="dirty-label">unsaved</span>{/if}
                    </div>

                    {#if selectedValueHidden}
                      <textarea class="line-editor" value={entry.displayValue} aria-label={`${entry.key} hidden value`} spellcheck="false" rows="2" readonly></textarea>
                    {:else}
                      <textarea class="line-editor" bind:value={editValue} aria-label={`Value for ${entry.key}`} spellcheck="false" rows="2"></textarea>
                    {/if}

                    {#if selectedEntryDuplicate}
                      <label class="inline-select" for="peek-duplicate-save-target">
                        <span>Duplicate save target</span>
                        <select
                          id="peek-duplicate-save-target"
                          value={duplicateSaveChoice(selectedFile, entry)}
                          onchange={(event) => setDuplicateSaveChoice(selectedFile, entry, event.currentTarget.value)}
                        >
                          <option value="">Choose target before saving</option>
                          <option value="this-occurrence">This occurrence only ({occurrenceLabel(selectedFile, entry)})</option>
                        </select>
                      </label>
                    {/if}

                    <div class="line-edit-actions">
                      <button
                        type="button"
                        class="compact-action"
                        disabled={notice.kind === 'loading' || !entry.shape.redactedByDefault}
                        onclick={() => void toggleSelectedReveal()}
                      >
                        {#if selectedEntryRevealed}
                          <EyeOff size={14} aria-hidden="true" />
                          <span>Hide value</span>
                        {:else}
                          <Eye size={14} aria-hidden="true" />
                          <span>Reveal value</span>
                        {/if}
                      </button>
                      <button class="compact-action primary" type="button" disabled={!canSave} onclick={() => void saveValue()}>
                        <Save size={14} aria-hidden="true" />
                        <span>Save change</span>
                      </button>
                      <button type="button" class="compact-action" disabled={selectedFileHasHiddenSecrets} onclick={() => (showRaw = !showRaw)}>
                        <span>{showRaw ? 'Hide raw file' : 'View raw file'}</span>
                      </button>
                      <span class="line-hint">{selectedEntryFindings[0]?.title ?? expectedShapeLabel(entry)} · {contractNote(selectedFile, entry)}</span>
                    </div>

                    {#if showRaw}
                      <pre>{selectedFile.content}</pre>
                    {/if}
                  </section>
                {/if}
              </div>
            {/each}
          {:else if selectedFile && filter.trim()}
            <section class="buffer-state compact">
              <Search size={20} aria-hidden="true" />
              <h2>No keys match “{filter.trim()}”</h2>
              <p>Try a key name, value type, “sensitive”, or exposure.</p>
              <button type="button" class="state-action" onclick={() => (filter = '')}>Clear filter</button>
            </section>
          {:else if selectedFile}
            <section class="buffer-state compact">
              <FileKey2 size={22} aria-hidden="true" />
              <h2>{selectedFile.name} has no parsed keys</h2>
              <p>Add the first KEY=value pair without leaving the project.</p>
              <button type="button" class="state-action primary" onclick={() => (showAddKey = true)}>
                <Plus size={15} aria-hidden="true" />
                Add first key
              </button>
            </section>
          {/if}
        </div>
      {:else if notice.kind === 'loading'}
        <section class="project-state" aria-busy="true">
          <span class="state-icon"><LoaderCircle class="spin" size={24} aria-hidden="true" /></span>
          <p class="eyebrow">Scanning project</p>
          <h1>Looking for environment files</h1>
          <code>{projectPath.trim() || '.'}</code>
          <p>Checking conventional filenames and env references in project configuration.</p>
        </section>
      {:else if notice.kind === 'error'}
        <section class="project-state error" role="alert">
          <span class="state-icon"><AlertTriangle size={24} aria-hidden="true" /></span>
          <p class="eyebrow">Project could not be opened</p>
          <h1>Check the folder and try again</h1>
          <code>{projectPath.trim() || '.'}</code>
          <p>{notice.message}</p>
          <div class="state-actions">
            <button type="button" class="state-action primary" disabled={!isTauriRuntime()} onclick={() => void browseProject()}>
              <FolderOpen size={15} aria-hidden="true" />
              Choose folder
            </button>
            <button type="button" class="state-action" onclick={() => void openProject()}>Try again</button>
          </div>
        </section>
      {:else if snapshot}
        <section class="project-state empty">
          <span class="state-icon"><FolderOpen size={24} aria-hidden="true" /></span>
          <p class="eyebrow">Folder scanned</p>
          <h1>No environment files in {projectLabel}</h1>
          <code>{projectRoot}</code>
          <p>vne checked common files such as <strong>.env</strong>, <strong>.env.local</strong>, and <strong>*.env</strong>, plus env files referenced by package and Compose configuration.</p>
          <div class="state-actions">
            <button type="button" class="state-action primary" disabled={!isTauriRuntime()} onclick={() => void browseProject()}>
              <FolderOpen size={15} aria-hidden="true" />
              Choose another folder
            </button>
            <button type="button" class="state-action" onclick={() => void openProject()}>
              <RefreshCw size={15} aria-hidden="true" />
              Scan again
            </button>
          </div>
        </section>
      {:else}
        <section class="project-state">
          <span class="state-icon"><FileKey2 size={24} aria-hidden="true" /></span>
          <p class="eyebrow">Environment workbench</p>
          <h1>Open a project folder</h1>
          <p>Inspect env files, understand key shapes, and make focused edits without exposing secrets.</p>
          <button type="button" class="state-action primary" disabled={!isTauriRuntime()} onclick={() => void browseProject()}>
            <FolderOpen size={15} aria-hidden="true" />
            Choose folder
          </button>
        </section>
      {/if}
    </section>

    {#if showReview && snapshot && findings.length > 0}
      <aside
        class="review-drawer"
        aria-label="Project findings"
        aria-modal={reviewIsModal ? 'true' : undefined}
        role={reviewIsModal ? 'dialog' : 'complementary'}
      >
        <header class="review-head">
          <div>
            <p class="eyebrow">Project review</p>
            <h2>{findings.length} finding{findings.length === 1 ? '' : 's'}</h2>
          </div>
          <button
            bind:this={reviewCloseButton}
            type="button"
            aria-label="Close project review"
            title="Close review"
            onclick={() => closeReview()}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>

        {#if missingKeyFindings.length > 0}
          <section class="review-section">
            <h3>Missing values</h3>
            <div class="finding-list">
              {#each missingKeyFindings as finding}
                <article class="finding-card warning">
                  <strong>{finding.title}</strong>
                  <p>{finding.detail}</p>
                  {#if finding.mutationPreview}<code>{finding.mutationPreview}</code>{/if}
                  <label>
                    <span>Value for {finding.key}</span>
                    <input
                      class="missing-key-value"
                      type={missingKeyInputType(finding)}
                      value={missingKeyDraft(finding)}
                      autocomplete="off"
                      spellcheck="false"
                      oninput={(event) => setMissingKeyDraft(finding, event.currentTarget.value)}
                    />
                  </label>
                  <button type="button" class="compact-action primary" disabled={!canAddMissingKey(finding)} onclick={() => void addMissingKey(finding)}>
                    <Plus size={14} aria-hidden="true" />
                    Add to {finding.filePath ? projectName(finding.filePath) : 'env file'}
                  </button>
                </article>
              {/each}
            </div>
          </section>
        {/if}

        {#if advisoryFindings.length > 0}
          <section class="review-section">
            <h3>Needs attention</h3>
            <div class="finding-list">
              {#each advisoryFindings as finding}
                <article class:warning={finding.severity === 'warning'} class="finding-card">
                  <strong>{finding.title}</strong>
                  <p>{finding.detail}</p>
                  {#if finding.evidence.length > 0}<small>{finding.evidence[0]}</small>{/if}
                  {#if canInspectFinding(finding)}
                    <button type="button" class="compact-action" onclick={() => openFinding(finding)}>Inspect key</button>
                  {/if}
                </article>
              {/each}
            </div>
          </section>
        {/if}

        <section class="review-section project-facts">
          <h3>Project context</h3>
          <dl>
            <div><dt>Files</dt><dd>{snapshot.files.length}</dd></div>
            <div><dt>Layers</dt><dd>{snapshot.layerReport.orderedFiles.length}</dd></div>
            <div><dt>Framework</dt><dd>{snapshot.frameworkProfiles[0]?.framework ?? 'Not detected'}</dd></div>
          </dl>
        </section>
      </aside>
    {/if}
  </div>

  <footer
    class="status-strip"
    class:error={notice.kind === 'error'}
    class:success={notice.kind === 'success'}
    role="status"
    aria-live="polite"
    aria-atomic="true"
    inert={showReview && reviewIsModal}
  >
    {#if notice.kind === 'loading'}
      <LoaderCircle class="spin" size={14} aria-hidden="true" />
    {:else if notice.kind === 'error'}
      <AlertTriangle size={14} aria-hidden="true" />
    {:else if notice.kind === 'success'}
      <CheckCircle2 size={14} aria-hidden="true" />
    {:else}
      <FileKey2 size={14} aria-hidden="true" />
    {/if}
    <span class="status-message">{notice.message}</span>
    {#if snapshot && snapshot.files.length > 0}
      <span class="sep"></span>
      <span class:quiet={attentionKeyCount === 0} class="attention-summary">
        <span class="mini-dot"></span>
        {attentionKeyCount === 0 ? `${selectedFile?.name ?? 'file'} is clear` : `${attentionKeyCount} key${attentionKeyCount === 1 ? '' : 's'} need attention`}
      </span>
      {#if snapshot.comparison}
        <span class="sep secondary-status"></span>
        <span class="secondary-status">
          {snapshot.comparison.sharedKeys.length}/{snapshot.comparison.sharedKeys.length + snapshot.comparison.missingKeys.length} contract ·
          {snapshot.comparison.missingKeys.length} missing · {snapshot.comparison.extraKeys.length} extra
        </span>
      {/if}
    {/if}
    <span class="spacer"></span>
    {#if selectedFile && visibleEntries.length > 0}
      <button type="button" onclick={() => moveSelection(-1)} disabled={selectedVisibleIndex <= 0}>prev</button>
      <button type="button" onclick={() => moveSelection(1)} disabled={selectedVisibleIndex >= visibleEntries.length - 1}>next</button>
      <span class="selection-status">{selectedVisibleIndex >= 0 && selectedEntry ? `ln ${selectedEntry.lineNumber} · ${selectedEntry.key}` : 'select a key to edit'}</span>
    {/if}
  </footer>
</main>
