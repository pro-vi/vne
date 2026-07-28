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
  import {
    canApplyTargetedOperation,
    createEditDraft,
    createEntryRef,
    createPendingOperation,
    draftForTarget,
    fileHasProtectedEntries,
    inputTypeForKey,
    isCurrentOperation,
    isDirtyDraft,
    isDuplicateSaveConfirmed,
    isReviewOpen as deriveReviewOpen,
    parseDuplicateSaveConfirmation,
    resolveFindingTarget,
    revealedValueFor,
    sameEntryRef,
    setDuplicateSaveConfirmation,
    updateEditDraft,
    type DuplicateSaveConfirmations,
    type EditDraft,
    type EntryRef,
    type OperationKind,
    type PendingOperation,
    type RevealedEntry
  } from './lib/workbench-state';

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

  let projectPath = '';
  let snapshot: ProjectSnapshot | null = null;
  let selectedPath = '';
  let selectedEntryId = '';
  let editorDraft: EditDraft | null = null;
  let filter = '';
  let filterInput: HTMLInputElement | null = null;
  let addKeyToggleButton: HTMLButtonElement | null = null;
  let reloadButton: HTMLButtonElement | null = null;
  let reviewToggleButton: HTMLButtonElement | null = null;
  let reviewCloseButton: HTMLButtonElement | null = null;
  let reviewReturnTarget: HTMLElement | null = null;
  let viewportWidth = 1180;
  let showRaw = false;
  let showAddKey = false;
  let reviewRequested = false;
  let wasReviewModalOpen = false;
  let newEnvKey = '';
  let newEnvValue = '';
  let showNewEnvValue = false;
  let missingKeyDrafts: Record<string, string> = {};
  let visibleMissingKeyValues: Record<string, boolean> = {};
  let duplicateSaveConfirmations: DuplicateSaveConfirmations = {};
  let revealedEntry: RevealedEntry | null = null;
  let pendingOperation: PendingOperation | null = null;
  let operationGeneration = 0;
  let notice: Notice = {
    kind: 'idle',
    message: isTauriRuntime() ? 'Open a project directory to inspect env files.' : 'Browser preview is using local sample data.'
  };

  $: selectedFile = snapshot?.files.find((file) => file.path === selectedPath) ?? snapshot?.files[0] ?? null;
  $: selectedEntry = selectedFile?.entries.find((entry) => entry.id === selectedEntryId) ?? null;
  $: activeEntryRef =
    snapshot && selectedFile && selectedEntry ? createEntryRef(snapshot.root, selectedFile, selectedEntry) : null;
  $: selectedDraft = draftForTarget(editorDraft, activeEntryRef);
  $: currentComparison = snapshot?.comparison ?? null;
  $: visibleEntries = selectedFile ? filterEntries(selectedFile, filter) : [];
  $: entryRows =
    snapshot && selectedFile ? buildEntryRows(snapshot.root, selectedFile, visibleEntries, currentComparison, revealedEntry) : [];
  $: selectedEntryValue =
    selectedEntry && activeEntryRef ? revealedValueFor(revealedEntry, activeEntryRef) ?? selectedEntry.value : '';
  $: selectedEntryRevealed = Boolean(activeEntryRef && revealedValueFor(revealedEntry, activeEntryRef) !== null);
  $: selectedValueHidden = selectedEntry ? isEntryValueHidden(selectedEntry, selectedEntryRevealed) : false;
  $: selectedEntryDuplicate = Boolean(selectedFile && selectedEntry && selectedFile.duplicateKeys.includes(selectedEntry.key));
  $: duplicateSaveAllowed = !selectedEntryDuplicate || isDuplicateSaveConfirmed(duplicateSaveConfirmations, activeEntryRef);
  $: normalizedNewEnvKey = newEnvKey.trim();
  $: newEnvDuplicateLines =
    selectedFile && normalizedNewEnvKey
      ? selectedFile.entries.filter((entry) => entry.key === normalizedNewEnvKey).map((entry) => entry.lineNumber)
      : [];
  $: newEnvValueType = inputTypeForKey(snapshot, normalizedNewEnvKey, showNewEnvValue);
  $: newEnvValueProtected = inputTypeForKey(snapshot, normalizedNewEnvKey) === 'password';
  $: selectedFileHasProtectedEntries = fileHasProtectedEntries(selectedFile);
  $: hasDirtyEdit = Boolean(selectedDraft && !selectedValueHidden && isDirtyDraft(selectedDraft));
  $: hasNewKeyDraft = Boolean(newEnvKey || newEnvValue);
  $: hasMissingKeyDrafts = Object.values(missingKeyDrafts).some((value) => value.length > 0);
  $: canSave = Boolean(
    isTauriRuntime() &&
      selectedFile &&
      selectedEntry &&
      selectedDraft &&
      !selectedValueHidden &&
      duplicateSaveAllowed &&
      selectedVisibleIndex >= 0 &&
      isDirtyDraft(selectedDraft)
  );
  $: canAddNewEnv = Boolean(
    isTauriRuntime() &&
      selectedFile &&
      normalizedNewEnvKey &&
      newEnvValue.trim().length > 0 &&
      newEnvDuplicateLines.length === 0
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
  $: isBusy = pendingOperation !== null;
  $: projectRoot = snapshot?.root ?? projectPath.trim();
  $: projectLabel = projectRoot ? projectName(projectRoot) : 'No project open';
  $: reviewIsModal = viewportWidth <= 980;
  $: reviewOpen = deriveReviewOpen(reviewRequested, snapshot);
  $: reviewModalOpen = reviewOpen && reviewIsModal;
  $: hiddenDirtyDraft = Boolean(hasDirtyEdit && selectedVisibleIndex === -1 && selectedEntry);
  $: if (selectedFileHasProtectedEntries && showRaw) {
    showRaw = false;
  }
  $: {
    const enteringModal = reviewModalOpen && !wasReviewModalOpen;
    wasReviewModalOpen = reviewModalOpen;
    if (enteringModal) {
      window.requestAnimationFrame(() => reviewCloseButton?.focus());
    }
  }

  function beginOperation(
    kind: OperationKind,
    message: string,
    target: EntryRef | null = null,
    draftRevision: number | null = null
  ): PendingOperation | null {
    if (pendingOperation) {
      return null;
    }

    const operation = createPendingOperation(++operationGeneration, kind, target, draftRevision);
    pendingOperation = operation;
    notice = { kind: 'loading', message };
    return operation;
  }

  function settleOperation(operation: PendingOperation, nextNotice: Notice): boolean {
    if (!isCurrentOperation(pendingOperation, operation)) {
      return false;
    }

    pendingOperation = null;
    notice = nextNotice;
    return true;
  }

  function applyLoadedProject(loaded: ProjectSnapshot): void {
    showRaw = false;
    showAddKey = false;
    reviewRequested = false;
    filter = '';
    newEnvKey = '';
    newEnvValue = '';
    showNewEnvValue = false;
    missingKeyDrafts = {};
    visibleMissingKeyValues = {};
    clearRevealedEntry();
    const firstFile = loaded.files[0];
    applySnapshotSelection(loaded, firstFile?.path ?? '', '', '');
  }

  async function loadProjectPath(requestedPath: string, confirmDiscard = true): Promise<boolean> {
    if (pendingOperation || !requestedPath || (confirmDiscard && !confirmDiscardedWork('project'))) {
      return false;
    }

    const operation = beginOperation('load-project', 'Scanning env files...');
    if (!operation) {
      return false;
    }

    try {
      const loaded = await loadProject(requestedPath);
      if (!isCurrentOperation(pendingOperation, operation)) {
        return false;
      }
      applyLoadedProject(loaded);
      settleOperation(operation, {
        kind: 'success',
        message: loaded.files.length
          ? `Loaded ${loaded.files.length} env file${loaded.files.length === 1 ? '' : 's'}.`
          : 'No env files found.'
      });
      return true;
    } catch (error) {
      settleOperation(operation, { kind: 'error', message: errorMessage(error) });
      return false;
    }
  }

  async function openProject(confirmDiscard = true): Promise<void> {
    const requestedPath = snapshot?.root ?? projectPath.trim();
    await loadProjectPath(requestedPath, confirmDiscard);
  }

  async function bootProject(): Promise<void> {
    try {
      const initialPath = await initialProjectPath();
      if (!isTauriRuntime()) {
        await loadProjectPath(initialPath ?? '.', false);
        return;
      }

      if (initialPath) {
        await loadProjectPath(initialPath, false);
        return;
      }

      notice = { kind: 'idle', message: 'Choose a project directory to inspect env files.' };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  async function browseProject(): Promise<void> {
    if (pendingOperation) {
      return;
    }

    if (!confirmDiscardedWork('project')) {
      return;
    }

    if (!isTauriRuntime()) {
      notice = { kind: 'error', message: 'Native folder picking is available in the Tauri desktop app.' };
      return;
    }

    const operation = beginOperation('browse-project', 'Choose a project folder...');
    if (!operation) {
      return;
    }

    try {
      const selected = await pickProjectDirectory(snapshot?.root ?? projectPath);
      if (!selected) {
        settleOperation(operation, { kind: 'idle', message: 'Folder selection cancelled.' });
        return;
      }

      notice = { kind: 'loading', message: 'Scanning selected folder...' };
      const loaded = await loadProject(selected);
      if (!isCurrentOperation(pendingOperation, operation)) {
        return;
      }
      applyLoadedProject(loaded);
      settleOperation(operation, {
        kind: 'success',
        message: loaded.files.length
          ? `Loaded ${loaded.files.length} env file${loaded.files.length === 1 ? '' : 's'}.`
          : 'No env files found.'
      });
    } catch (error) {
      settleOperation(operation, { kind: 'error', message: errorMessage(error) });
    }
  }

  function chooseFile(file: EnvFile): void {
    if (pendingOperation || file.path === selectedPath || !confirmDiscardedWork('file')) {
      return;
    }

    const revealedKey = revealedEntry?.target.key ?? null;
    clearRevealedEntry();
    if (revealedKey) {
      notice = { kind: 'success', message: `${revealedKey} hidden after file selection changed.` };
    }
    showRaw = false;
    showAddKey = false;
    newEnvKey = '';
    newEnvValue = '';
    showNewEnvValue = false;
    selectedPath = file.path;
    selectedEntryId = '';
    editorDraft = null;
  }

  function chooseEntry(file: EnvFile, entry: EnvEntry, confirmDiscard = true): boolean {
    if (pendingOperation || !snapshot) {
      return false;
    }

    const target = createEntryRef(snapshot.root, file, entry);
    if (sameEntryRef(target, activeEntryRef)) {
      return true;
    }

    if (confirmDiscard && !confirmDiscardedWork('entry')) {
      return false;
    }

    const revealedKey = revealedEntry?.target.key ?? null;
    if (!sameEntryRef(target, revealedEntry?.target)) {
      clearRevealedEntry();
      if (revealedKey) {
        notice = { kind: 'success', message: `${revealedKey} hidden after selection changed.` };
      }
    }
    selectedPath = file.path;
    selectedEntryId = entry.id;
    editorDraft = createEditDraft(target, revealedValueFor(revealedEntry, target) ?? entry.value);
    return true;
  }

  function moveSelection(delta: number): void {
    if (pendingOperation || !selectedFile || visibleEntries.length === 0) {
      return;
    }

    const foundIndex = visibleEntries.findIndex((entry) => entry.id === selectedEntryId);
    const currentIndex = foundIndex === -1 ? (delta > 0 ? -1 : 0) : foundIndex;
    const nextIndex = Math.min(Math.max(currentIndex + delta, 0), visibleEntries.length - 1);
    if (!chooseEntry(selectedFile, visibleEntries[nextIndex])) {
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

    if (event.key === 'Escape' && reviewOpen) {
      event.preventDefault();
      if (!pendingOperation) {
        closeReview();
      }
      return;
    }

    if (reviewModalOpen) {
      return;
    }

    if (event.key === '/' && !isEditing && !pendingOperation) {
      event.preventDefault();
      filterInput?.focus();
    }

    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      if (canSave && !pendingOperation) {
        void saveValue();
      }
    }

    if (event.key === 'Escape' && filter) {
      event.preventDefault();
      clearFilter();
    }
  }

  async function saveValue(): Promise<void> {
    if (!selectedFile || !selectedEntry || !selectedDraft || !canSave || pendingOperation) {
      return;
    }

    const draft = selectedDraft;
    const operation = beginOperation('save-entry', `Saving ${draft.target.key}...`, draft.target, draft.revision);
    if (!operation) {
      return;
    }

    try {
      const saved = await saveEnvValue(
        draft.target.root,
        draft.target.filePath,
        draft.target.key,
        draft.target.lineNumber,
        draft.value
      );
      if (!canApplyTargetedOperation(pendingOperation, operation, activeEntryRef)) {
        settleOperation(operation, {
          kind: 'success',
          message: `${draft.target.key} saved. Reload to see the refreshed project state.`
        });
        return;
      }

      applySnapshotSelection(saved, draft.target.filePath, draft.target.entryId, draft.target.key);
      settleOperation(operation, {
        kind: 'success',
        message: `${draft.target.key} saved and project diagnostics rescanned.`
      });
      window.requestAnimationFrame(() => focusSelectedEntry());
    } catch (error) {
      settleOperation(operation, { kind: 'error', message: errorMessage(error) });
    }
  }

  async function addMissingKey(finding: EnvFinding): Promise<void> {
    if (!canAddMissingKey(finding, missingKeyDrafts) || !finding.filePath || !finding.key || pendingOperation) {
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

    const value = missingKeyDraft(finding, missingKeyDrafts).trim();
    const filePath = finding.filePath;
    const key = finding.key;
    const operation = beginOperation('add-entry', `Adding ${key}...`);
    if (!operation) {
      return;
    }

    try {
      const loaded = await addEnvKey(currentProjectRoot(), filePath, key, value);
      if (!isCurrentOperation(pendingOperation, operation)) {
        return;
      }
      deleteMissingKeyDraft(finding);
      deleteVisibleMissingKeyValue(finding);
      filter = '';
      applySnapshotSelection(loaded, filePath, '', key);
      closeReview(false);
      settleOperation(operation, { kind: 'success', message: `${key} was added and project diagnostics rescanned.` });
      window.requestAnimationFrame(() => focusSelectedEntry());
    } catch (error) {
      settleOperation(operation, { kind: 'error', message: errorMessage(error) });
    }
  }

  async function addNewEnv(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!selectedFile || !normalizedNewEnvKey || pendingOperation) {
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
    const operation = beginOperation('add-entry', `Adding ${key}...`);
    if (!operation) {
      return;
    }

    try {
      const loaded = await addEnvKey(currentProjectRoot(), targetPath, key, value);
      if (!isCurrentOperation(pendingOperation, operation)) {
        return;
      }
      newEnvKey = '';
      newEnvValue = '';
      showNewEnvValue = false;
      filter = '';
      applySnapshotSelection(loaded, targetPath, '', key);
      showAddKey = false;
      settleOperation(operation, { kind: 'success', message: `${key} was added and project diagnostics rescanned.` });
      window.requestAnimationFrame(() => focusSelectedEntry());
    } catch (error) {
      settleOperation(operation, { kind: 'error', message: errorMessage(error) });
    }
  }

  async function toggleSelectedReveal(): Promise<void> {
    if (!snapshot || !selectedFile || !selectedEntry || !activeEntryRef || !selectedEntry.shape.redactedByDefault || pendingOperation) {
      return;
    }

    if (selectedEntryRevealed) {
      if (!confirmDiscardedWork('entry')) {
        return;
      }
      clearRevealedEntry();
      editorDraft = createEditDraft(activeEntryRef, selectedEntry.value);
      showRaw = false;
      notice = { kind: 'success', message: `${selectedEntry.key} hidden.` };
      return;
    }

    const target = activeEntryRef;
    const operation = beginOperation('reveal-entry', `Revealing ${target.key}...`, target);
    if (!operation) {
      return;
    }

    try {
      const value = await revealEnvValue(target.root, target.filePath, target.key, target.lineNumber);
      if (!canApplyTargetedOperation(pendingOperation, operation, activeEntryRef)) {
        settleOperation(operation, { kind: 'idle', message: `${target.key} reveal was discarded after the selection changed.` });
        return;
      }

      revealedEntry = { target, value };
      editorDraft = createEditDraft(target, value);
      settleOperation(operation, {
        kind: 'success',
        message: `${target.key} revealed until selection, hide, reload, or save.`
      });
    } catch (error) {
      settleOperation(operation, { kind: 'error', message: errorMessage(error) });
    }
  }

  function inspectFinding(finding: EnvFinding): boolean {
    if (!snapshot || pendingOperation) {
      return false;
    }

    const resolved = resolveFindingTarget(snapshot, finding);
    if (!resolved) {
      notice = { kind: 'error', message: `Could not find ${finding.key ?? 'that key'} in the current snapshot.` };
      return false;
    }

    const changesFile = resolved.file.path !== selectedPath;
    if (!confirmDiscardedWork(changesFile ? 'file' : 'entry')) {
      return false;
    }

    if (changesFile) {
      showAddKey = false;
      newEnvKey = '';
      newEnvValue = '';
    }

    filter = '';
    return chooseEntry(resolved.file, resolved.entry, false);
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
    if (pendingOperation) {
      return;
    }

    if (reviewOpen) {
      closeReview();
      return;
    }

    reviewReturnTarget = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    reviewRequested = true;
    window.requestAnimationFrame(() => reviewCloseButton?.focus());
  }

  function closeReview(restoreFocus = true): void {
    reviewRequested = false;
    if (restoreFocus) {
      window.requestAnimationFrame(() => focusReviewReturnTarget());
    }
  }

  function isAddMissingKeyFinding(finding: EnvFinding): boolean {
    return finding.actionKind === 'add-missing-key' && Boolean(finding.filePath && finding.key);
  }

  function canAddMissingKey(finding: EnvFinding, drafts: Record<string, string>): boolean {
    return (
      isTauriRuntime() &&
      isAddMissingKeyFinding(finding) &&
      missingKeyDraft(finding, drafts).trim().length > 0
    );
  }

  function canInspectFinding(currentSnapshot: ProjectSnapshot | null, finding: EnvFinding): boolean {
    return Boolean(currentSnapshot && resolveFindingTarget(currentSnapshot, finding));
  }

  function currentProjectRoot(): string {
    return snapshot?.root ?? projectPath.trim();
  }

  function missingKeyDraft(finding: EnvFinding, drafts: Record<string, string>): string {
    return drafts[missingKeyDraftRef(finding)] ?? '';
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

  function missingKeyInputType(
    currentSnapshot: ProjectSnapshot | null,
    finding: EnvFinding,
    visibility: Record<string, boolean>
  ): 'password' | 'text' {
    return inputTypeForKey(currentSnapshot, finding.key ?? '', isMissingKeyValueVisible(finding, visibility));
  }

  function isMissingKeyValueVisible(finding: EnvFinding, visibility: Record<string, boolean>): boolean {
    return Boolean(visibility[missingKeyDraftRef(finding)]);
  }

  function toggleMissingKeyValue(finding: EnvFinding): void {
    const key = missingKeyDraftRef(finding);
    visibleMissingKeyValues = { ...visibleMissingKeyValues, [key]: !visibleMissingKeyValues[key] };
  }

  function deleteVisibleMissingKeyValue(finding: EnvFinding): void {
    const next = { ...visibleMissingKeyValues };
    delete next[missingKeyDraftRef(finding)];
    visibleMissingKeyValues = next;
  }

  function duplicateLineLabel(lines: number[]): string {
    return `${lines.length === 1 ? 'line' : 'lines'} ${lines.join(', ')}`;
  }

  function duplicateSaveChoice(target: EntryRef | null, confirmations: DuplicateSaveConfirmations): string {
    return isDuplicateSaveConfirmed(confirmations, target) ? 'this-occurrence' : '';
  }

  function setDuplicateSaveChoice(file: EnvFile, entry: EnvEntry, value: string): void {
    if (!snapshot) {
      return;
    }
    const target = createEntryRef(snapshot.root, file, entry);
    duplicateSaveConfirmations = setDuplicateSaveConfirmation(
      duplicateSaveConfirmations,
      target,
      parseDuplicateSaveConfirmation(value)
    );
  }

  function applySnapshotSelection(loaded: ProjectSnapshot, preferredPath: string, preferredEntryId: string, preferredKey: string): void {
    clearRevealedEntry();
    duplicateSaveConfirmations = {};
    snapshot = loaded;
    projectPath = loaded.root;
    if (loaded.findings.length === 0) {
      reviewRequested = false;
    }
    const refreshedFile = loaded.files.find((file) => file.path === preferredPath) ?? loaded.files[0] ?? null;
    selectedPath = refreshedFile?.path ?? preferredPath;
    const refreshedEntry =
      (preferredEntryId ? refreshedFile?.entries.find((entry) => entry.id === preferredEntryId) : undefined) ??
      (preferredKey ? refreshedFile?.entries.find((entry) => entry.key === preferredKey) : undefined) ??
      null;
    selectedEntryId = refreshedEntry?.id ?? '';
    editorDraft = refreshedFile && refreshedEntry ? createEditDraft(createEntryRef(loaded.root, refreshedFile, refreshedEntry), refreshedEntry.value) : null;
  }

  function clearRevealedEntry(): void {
    revealedEntry = null;
  }

  function updateEditorValue(value: string): void {
    if (pendingOperation || !selectedDraft) {
      return;
    }
    editorDraft = updateEditDraft(selectedDraft, value);
  }

  function toggleAddKeyForm(): void {
    if (!pendingOperation) {
      showAddKey = !showAddKey;
    }
  }

  function closeAddKeyForm(): void {
    if (pendingOperation) {
      return;
    }

    showAddKey = false;
    window.requestAnimationFrame(() => addKeyToggleButton?.focus({ preventScroll: true }));
  }

  function focusSelectedEntry(): void {
    document.querySelector<HTMLButtonElement>('.line.sel')?.focus({ preventScroll: true });
  }

  function clearFilter(): void {
    if (pendingOperation) {
      return;
    }

    filter = '';
    window.requestAnimationFrame(() => filterInput?.focus({ preventScroll: true }));
  }

  function showHiddenDraft(): void {
    if (pendingOperation) {
      return;
    }

    filter = '';
    window.requestAnimationFrame(() => {
      const editor = document.querySelector<HTMLTextAreaElement>('.row-wrap.open .line-editor');
      editor?.focus({ preventScroll: true });
    });
  }

  function focusReviewReturnTarget(): void {
    const candidates = [
      reviewReturnTarget,
      reviewToggleButton,
      document.querySelector<HTMLButtonElement>('.line.sel'),
      filterInput,
      reloadButton
    ];
    const target = candidates.find(
      (candidate): candidate is HTMLElement => Boolean(candidate?.isConnected && !candidate.closest('[inert]'))
    );
    target?.focus({ preventScroll: true });
    reviewReturnTarget = null;
  }

  function rowAriaLabel(row: EntryRow): string {
    const valueProtected = row.entry.shape.sensitive || row.entry.shape.redactedByDefault;
    const value = row.valueHidden || valueProtected ? '' : `, value ${row.displayValue}`;
    return `${row.entry.key}, line ${row.entry.lineNumber}${value}, ${row.shapeLabel}, ${row.attentionLabel || statusLabel(row.status)}`;
  }

  function occurrenceLabel(file: EnvFile, entry: EnvEntry): string {
    const occurrences = file.entries.filter((candidate) => candidate.key === entry.key);
    const index = occurrences.findIndex((candidate) => candidate.id === entry.id);
    return `${index === -1 ? 1 : index + 1} of ${occurrences.length || 1}`;
  }

  function exposureLabel(entry: EnvEntry): string {
    if (entry.shape.exposure === 'browser') {
      return 'browser-exposed';
    }
    return entry.shape.exposure === null ? 'local process' : 'exposure unknown';
  }

  function buildEntryRows(
    root: string,
    file: EnvFile,
    entries: EnvEntry[],
    comparison: EnvComparison | null,
    revealed: RevealedEntry | null
  ): EntryRow[] {
    return entries.map((entry) => {
      const target = createEntryRef(root, file, entry);
      const status = keyStatus(file, entry, comparison);
      const attentionClass = entryAttentionClass(entry, status);
      const revealedValue = revealedValueFor(revealed, target);
      const valueHidden = isEntryValueHidden(entry, revealedValue !== null);

      return {
        entry,
        status,
        attentionClass,
        attentionLabel: attentionClass ? entryAttentionLabel(entry, status) : '',
        categoryClass: entryCategoryClass(entry),
        shapeLabel: shapeShortLabel(entry),
        displayValue: valueHidden ? entry.displayValue : (revealedValue ?? entry.value) || '(empty)',
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

  function contractNote(comparison: EnvComparison | null, file: EnvFile, entry: EnvEntry): string {
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

<main class="app-shell" class:review-modal-open={reviewModalOpen} aria-busy={isBusy}>
  <header class="chrome" inert={reviewModalOpen}>
    <div class="project-lockup" title={projectRoot}>
      <span class="app-mark" title="vne"><FileKey2 size={16} aria-hidden="true" /></span>
      <span class="project-identity">
        <strong>{projectLabel}</strong>
        <span>{projectRoot || 'Choose a folder to begin'}</span>
      </span>
    </div>

    <div class="project-actions">
      <button
        type="button"
        class="chrome-action"
        title={isTauriRuntime() ? 'Choose another project folder' : 'Folder picking requires the desktop app'}
        disabled={!isTauriRuntime()}
        aria-disabled={isBusy}
        onclick={() => void browseProject()}
      >
        <FolderOpen size={15} aria-hidden="true" />
        <span>Change folder</span>
      </button>
      <button
        bind:this={reloadButton}
        type="button"
        class="chrome-action icon-only"
        title="Reload project"
        aria-label="Reload project"
        disabled={!snapshot && !projectPath}
        aria-disabled={isBusy}
        onclick={() => void openProject()}
      >
        <RefreshCw size={15} aria-hidden="true" />
      </button>
    </div>
  </header>

  {#if reviewModalOpen}<div class="modal-scrim" aria-hidden="true"></div>{/if}

  <div
    class:with-review={reviewOpen}
    class:review-overlay={reviewModalOpen}
    class="semantic-main"
  >
    <section class="editor" aria-label="Environment workspace" inert={reviewModalOpen} aria-busy={isBusy}>
      {#if snapshot && snapshot.files.length > 0}
        <div class="workspace-bar">
          <nav class="file-tabs" aria-label="Environment files">
            {#each snapshot.files as file}
              <button
                class:active={file.path === selectedPath}
                type="button"
                aria-pressed={file.path === selectedPath}
                aria-disabled={isBusy}
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
              readonly={isBusy}
            />
            {#if filter}
              <button
                type="button"
                aria-label="Clear key filter"
                title="Clear filter"
                aria-disabled={isBusy}
                onclick={clearFilter}
              >
                <X size={13} aria-hidden="true" />
              </button>
            {/if}
          </label>

          <button
            bind:this={addKeyToggleButton}
            type="button"
            class:on={showAddKey}
            class="toolbar-action"
            aria-expanded={showAddKey}
            aria-disabled={isBusy}
            onclick={toggleAddKeyForm}
          >
            <Plus size={14} aria-hidden="true" />
            <span>Add key</span>
            {#if hasNewKeyDraft}<span class="draft-badge">draft</span>{/if}
          </button>
        </div>

        {#if notice.kind === 'error'}
          <div class="inline-notice error" role="alert">
            <AlertTriangle size={15} aria-hidden="true" />
            <span>{notice.message} The loaded project is still shown below.</span>
            <button type="button" aria-disabled={isBusy} onclick={() => void openProject()}>Try again</button>
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
              class:on={reviewOpen}
              class="review-toggle"
              aria-expanded={reviewOpen}
              aria-disabled={isBusy}
              onclick={toggleReview}
            >
              {reviewOpen ? 'Hide review' : 'Review findings'}
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
              readonly={isBusy}
            />
            <input
              class="new-env-value"
              aria-label="New env value"
              bind:value={newEnvValue}
              type={newEnvValueType}
              placeholder="value"
              autocomplete="off"
              spellcheck="false"
              readonly={isBusy}
            />
            {#if newEnvValueProtected}
              <button
                type="button"
                class="compact-action"
                aria-pressed={showNewEnvValue}
                aria-disabled={isBusy}
                onclick={() => {
                  if (!pendingOperation) showNewEnvValue = !showNewEnvValue;
                }}
              >
                {#if showNewEnvValue}<EyeOff size={14} aria-hidden="true" />{:else}<Eye size={14} aria-hidden="true" />{/if}
                <span>{showNewEnvValue ? 'Hide' : 'Show'} value</span>
              </button>
            {/if}
            <button class="compact-action primary" type="submit" disabled={!canAddNewEnv} aria-disabled={isBusy}>
              <Plus size={14} aria-hidden="true" />
              <span>Add</span>
            </button>
            <button
              type="button"
              class="compact-action"
              aria-label="Close add key form; entered draft is retained"
              aria-disabled={isBusy}
              onclick={closeAddKeyForm}
            >
              <X size={14} aria-hidden="true" />
            </button>
            {#if newEnvDuplicateLines.length > 0}
              <span class="duplicate-note">{normalizedNewEnvKey} already exists at {duplicateLineLabel(newEnvDuplicateLines)}.</span>
            {/if}
          </form>
        {/if}

        {#if hiddenDirtyDraft && selectedEntry}
          <section class="hidden-draft" role="status">
            <span><strong>Unsaved {selectedEntry.key}</strong> is hidden by the current filter.</span>
            <button
              type="button"
              class="compact-action"
              aria-disabled={isBusy}
              onclick={showHiddenDraft}
            >Show draft</button>
          </section>
        {/if}

        <div
          class="buffer"
          role={selectedFile && visibleEntries.length > 0 ? 'list' : undefined}
          aria-label={selectedFile && visibleEntries.length > 0 ? `Parsed keys in ${selectedFile.name}` : undefined}
        >
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
                  aria-label={rowAriaLabel(row)}
                  aria-disabled={isBusy}
                  onclick={() => selectedFile && chooseEntry(selectedFile, entry)}
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
                      <textarea
                        class="line-editor"
                        value={selectedDraft?.value ?? ''}
                        aria-label={`Value for ${entry.key}`}
                        spellcheck="false"
                        rows="2"
                        readonly={isBusy}
                        oninput={(event) => updateEditorValue(event.currentTarget.value)}
                      ></textarea>
                    {/if}

                    {#if selectedEntryDuplicate}
                      <label class="inline-select" for="peek-duplicate-save-target">
                        <span>Duplicate save target</span>
                        <select
                          id="peek-duplicate-save-target"
                          value={duplicateSaveChoice(activeEntryRef, duplicateSaveConfirmations)}
                          disabled={isBusy}
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
                        disabled={!entry.shape.redactedByDefault}
                        aria-disabled={isBusy}
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
                      <button class="compact-action primary" type="button" disabled={!canSave} aria-disabled={isBusy} onclick={() => void saveValue()}>
                        <Save size={14} aria-hidden="true" />
                        <span>Save change</span>
                      </button>
                      <button
                        type="button"
                        class="compact-action"
                        disabled={selectedFileHasProtectedEntries}
                        title={selectedFileHasProtectedEntries ? 'Raw preview is unavailable while this file contains protected entries' : 'Toggle raw file preview'}
                        onclick={() => (showRaw = !showRaw)}
                      >
                        <span>{showRaw ? 'Hide raw file' : 'View raw file'}</span>
                      </button>
                      <span class="line-hint">{selectedEntryFindings[0]?.title ?? expectedShapeLabel(entry)} · {contractNote(currentComparison, selectedFile, entry)}</span>
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
              <button type="button" class="state-action" aria-disabled={isBusy} onclick={clearFilter}>Clear filter</button>
            </section>
          {:else if selectedFile}
            <section class="buffer-state compact">
              <FileKey2 size={22} aria-hidden="true" />
              <h2>{selectedFile.name} has no parsed keys</h2>
              <p>Add the first KEY=value pair without leaving the project.</p>
              <button type="button" class="state-action primary" aria-disabled={isBusy} onclick={toggleAddKeyForm}>
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
          <code>{projectPath.trim() || 'selected folder'}</code>
          <p>Checking conventional filenames and env references in project configuration.</p>
        </section>
      {:else if notice.kind === 'error'}
        <section class="project-state error" role="alert">
          <span class="state-icon"><AlertTriangle size={24} aria-hidden="true" /></span>
          <p class="eyebrow">Project could not be opened</p>
          <h1>Check the folder and try again</h1>
          {#if projectPath}<code>{projectPath}</code>{/if}
          <p>{notice.message}</p>
          <div class="state-actions">
            <button type="button" class="state-action primary" disabled={!isTauriRuntime()} aria-disabled={isBusy} onclick={() => void browseProject()}>
              <FolderOpen size={15} aria-hidden="true" />
              Choose folder
            </button>
            {#if snapshot || projectPath}
              <button type="button" class="state-action" aria-disabled={isBusy} onclick={() => void openProject()}>Try again</button>
            {/if}
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
            <button type="button" class="state-action primary" disabled={!isTauriRuntime()} aria-disabled={isBusy} onclick={() => void browseProject()}>
              <FolderOpen size={15} aria-hidden="true" />
              Choose another folder
            </button>
            <button type="button" class="state-action" aria-disabled={isBusy} onclick={() => void openProject()}>
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
          <button type="button" class="state-action primary" disabled={!isTauriRuntime()} aria-disabled={isBusy} onclick={() => void browseProject()}>
            <FolderOpen size={15} aria-hidden="true" />
            Choose folder
          </button>
        </section>
      {/if}
    </section>

    {#if reviewOpen && snapshot}
      <aside
        class="review-drawer"
        aria-label="Project findings"
        aria-modal={reviewModalOpen ? 'true' : undefined}
        role={reviewModalOpen ? 'dialog' : 'complementary'}
        aria-busy={isBusy}
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
            aria-disabled={isBusy}
            onclick={() => {
              if (!pendingOperation) closeReview();
            }}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>

        {#if reviewModalOpen && notice.kind !== 'idle'}
          <div
            class="review-notice"
            class:error={notice.kind === 'error'}
            role={notice.kind === 'error' ? 'alert' : 'status'}
            aria-live={notice.kind === 'error' ? 'assertive' : 'polite'}
            aria-atomic="true"
          >
            {#if notice.kind === 'loading'}
              <LoaderCircle class="spin" size={14} aria-hidden="true" />
            {:else if notice.kind === 'error'}
              <AlertTriangle size={14} aria-hidden="true" />
            {:else}
              <CheckCircle2 size={14} aria-hidden="true" />
            {/if}
            <span>{notice.message}</span>
          </div>
        {/if}

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
                      type={missingKeyInputType(snapshot, finding, visibleMissingKeyValues)}
                      value={missingKeyDraft(finding, missingKeyDrafts)}
                      autocomplete="off"
                      spellcheck="false"
                      readonly={isBusy}
                      oninput={(event) => setMissingKeyDraft(finding, event.currentTarget.value)}
                    />
                  </label>
                  {#if inputTypeForKey(snapshot, finding.key ?? '') === 'password'}
                    <button
                      type="button"
                      class="compact-action"
                      aria-pressed={isMissingKeyValueVisible(finding, visibleMissingKeyValues)}
                      aria-disabled={isBusy}
                      onclick={() => {
                        if (!pendingOperation) toggleMissingKeyValue(finding);
                      }}
                    >
                      {#if isMissingKeyValueVisible(finding, visibleMissingKeyValues)}<EyeOff size={14} aria-hidden="true" />{:else}<Eye size={14} aria-hidden="true" />{/if}
                      {isMissingKeyValueVisible(finding, visibleMissingKeyValues) ? 'Hide value' : 'Show value'}
                    </button>
                  {/if}
                  <button
                    type="button"
                    class="compact-action primary"
                    disabled={!canAddMissingKey(finding, missingKeyDrafts)}
                    aria-disabled={isBusy}
                    onclick={() => void addMissingKey(finding)}
                  >
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
                  {#if canInspectFinding(snapshot, finding)}
                    <button type="button" class="compact-action" aria-disabled={isBusy} onclick={() => openFinding(finding)}>Inspect key</button>
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
    inert={reviewModalOpen}
  >
    <span
      class="status-live"
      role={notice.kind === 'error' ? 'alert' : 'status'}
      aria-live={notice.kind === 'error' ? 'assertive' : 'polite'}
      aria-atomic="true"
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
    </span>
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
      <button type="button" onclick={() => moveSelection(-1)} disabled={selectedVisibleIndex <= 0 || isBusy}>prev</button>
      <button type="button" onclick={() => moveSelection(1)} disabled={selectedVisibleIndex >= visibleEntries.length - 1 || isBusy}>next</button>
      <span class="selection-status">{selectedVisibleIndex >= 0 && selectedEntry ? `ln ${selectedEntry.lineNumber} · ${selectedEntry.key}` : 'select a key to edit'}</span>
    {/if}
  </footer>
</main>
