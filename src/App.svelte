<script lang="ts">
  import { onMount, tick } from 'svelte';
  import {
    addEnvKey,
    copyEnvValue,
    createEnvFromExample,
    ensureEnvFile,
    initialProjectPath,
    isTauriRuntime,
    loadProject,
    onConcealedCopyExpired,
    pickProjectDirectory,
    revealEnvValue,
    saveEnvValue
  } from './lib/tauri';
  import { baseName } from './lib/paths';
  import type { EnvEntry, EnvFile, ProjectSnapshot } from './lib/types';
  import {
    ADD_ROW_ID,
    buildRows,
    duplicateHint,
    findExampleToCopy,
    initialSelection,
    isExampleFile,
    needCount,
    nextNeedingIndex,
    type NoteTone,
    type Row
  } from './lib/rows';
  import {
    canApplyTargetedOperation,
    createEntryRef,
    createPendingOperation,
    inputTypeForKey,
    isCurrentOperation,
    requiresProtectedInput,
    type EntryRef,
    type OperationKind,
    type PendingOperation,
    type RevealedEntry
  } from './lib/workbench-state';

  type MessageTone = 'ok' | 'warning' | 'danger' | 'info';
  type Message = { tone: MessageTone; text: string };
  type Verb = { shortcut: string; label: string; run: () => void; hold?: boolean };
  type Tab = { id: string; label: string; count: number | null; badge: string; badgeTone: 'danger' | 'warning' | ''; ghost: boolean };
  type Selection = { path: string; rowId: string };

  /** The one row being typed into. `target` is set for an existing entry;
   * a missing or new key is appended instead. */
  type Edit = {
    rowId: string;
    filePath: string;
    target: EntryRef | null;
    phase: 'name' | 'value';
    key: string;
    value: string;
    baseline: string;
    lines: number;
    needed: boolean;
    takenRowId: string;
    refused: boolean;
  };

  const NEW_ENV_TAB = '\u0000fresh';
  const UNREADABLE_TAB = '\u0000unread:';
  /** Same rule as `is_valid_env_key` in src-tauri/src/lib.rs. */
  const VALID_KEY_NAME = /^[A-Za-z_][A-Za-z0-9_.-]*$/;

  let projectPath = '';
  let snapshot: ProjectSnapshot | null = null;
  let loading = false;
  let loadError = '';
  let selectedPath = '';
  let selectedRowId = '';
  let edit: Edit | null = null;
  let revealed: RevealedEntry | null = null;
  let optionHeld = false;
  let message: Message | null = null;
  let savedRowId = '';
  /** The concealed copy whose clear the window is counting down to. */
  let pendingClear: { key: string; copyId: number; endsAt: number } | null = null;
  /** The reload started when the window regains focus; actions wait for it
   * instead of being dropped while it holds the pending operation. */
  let quietLoad: Promise<void> | null = null;
  let promptMessage: Message | null = null;
  let now = Date.now();
  let pendingOperation: PendingOperation | null = null;
  let operationGeneration = 0;
  let nameInput: HTMLInputElement | null = null;
  let valueInput: HTMLInputElement | null = null;
  let buffer: HTMLElement | null = null;

  $: exampleToCopy = snapshot ? findExampleToCopy(snapshot) : null;
  $: tabs = buildTabs(snapshot, exampleToCopy);
  $: selectedFile = snapshot?.files.find((file) => file.path === selectedPath) ?? null;
  $: rows = snapshot && selectedFile ? buildRows(snapshot, selectedFile) : [];
  $: selectedIndex = rows.findIndex((row) => row.id === selectedRowId);
  $: selectedRow = selectedIndex >= 0 ? rows[selectedIndex] : null;
  $: selectedTarget = targetFor(snapshot, selectedFile, selectedRow);
  $: needs = needCount(rows);
  $: tracked = selectedFile?.gitStatus === 'tracked';
  // Template only: inside handlers the reactive copy can lag a just-settled
  // operation, so they read pendingOperation directly.
  $: isBusy = pendingOperation !== null;
  $: unreadable = selectedPath.startsWith(UNREADABLE_TAB)
    ? snapshot?.incomplete.find((item) => UNREADABLE_TAB + item.name === selectedPath) ?? null
    : null;
  $: newEnvNeeds = exampleToCopy ? exampleToCopy.entries.filter((entry) => entry.valueState !== 'set').length : 0;
  $: secondsUntilClear = pendingClear ? Math.max(0, Math.ceil((pendingClear.endsAt - now) / 1000)) : 0;
  $: promptMessage = message ?? (pendingClear ? { tone: 'ok', text: `✓ copied ${pendingClear.key} · clipboard clears in ${secondsUntilClear} s` } : null);
  $: verbs = verbsFor(snapshot, selectedPath, selectedRow, rows, edit, tracked, exampleToCopy, loadError);
  $: hint = hintFor(snapshot, selectedFile, selectedRow, edit, revealed);
  $: if (selectedRowId && buffer) void scrollSelectedIntoView();

  function buildTabs(current: ProjectSnapshot | null, example: EnvFile | null): Tab[] {
    if (!current) {
      return [];
    }
    const freshTab: Tab[] = example
      ? [{ id: NEW_ENV_TAB, label: '.env', count: null, badge: 'new', badgeTone: 'warning', ghost: true }]
      : [];
    const fileTabs: Tab[] = current.files.map((file) => ({
      id: file.path,
      label: file.name,
      count: file.entries.length,
      // A template is meant to be committed; only a real env file is at risk.
      badge: file.gitStatus === 'tracked' && !isExampleFile(file) ? 'tracked' : '',
      badgeTone: 'danger',
      ghost: false
    }));
    const unreadTabs: Tab[] = current.incomplete.map((item) => ({
      id: UNREADABLE_TAB + item.name,
      label: item.name,
      count: null,
      badge: 'not read',
      badgeTone: 'warning',
      ghost: true
    }));
    return [...freshTab, ...fileTabs, ...unreadTabs];
  }

  function targetFor(current: ProjectSnapshot | null, file: EnvFile | null, row: Row | null): EntryRef | null {
    return current && file && row?.kind === 'entry' ? createEntryRef(current.root, file, row.entry) : null;
  }

  function verbsFor(
    current: ProjectSnapshot | null,
    path: string,
    row: Row | null,
    allRows: Row[],
    active: Edit | null,
    inTrackedFile: boolean,
    example: EnvFile | null,
    error: string
  ): Verb[] {
    if (!current) {
      return error
        ? [{ shortcut: '⌘O', label: 'choose folder', run: browse }, { shortcut: '⌘R', label: 'try again', run: reload }]
        : [{ shortcut: '⌘O', label: 'choose folder', run: browse }];
    }
    if (active) {
      if (active.refused) {
        return [
          { shortcut: '⌘R', label: 'reload, keep what you typed', run: reload },
          { shortcut: 'esc', label: 'cancel', run: cancelEdit }
        ];
      }
      if (active.takenRowId) {
        const line = allRows.find((candidate) => candidate.id === active.takenRowId);
        const lineNumber = line?.kind === 'entry' ? line.entry.lineNumber : '';
        return [
          { shortcut: '⏎', label: `edit line ${lineNumber}`, run: commitEdit },
          { shortcut: 'esc', label: 'cancel', run: cancelEdit }
        ];
      }
      if (active.phase === 'name') {
        return [
          { shortcut: '⏎', label: 'next', run: commitEdit },
          { shortcut: 'esc', label: 'cancel', run: cancelEdit }
        ];
      }
      const list: Verb[] = [{ shortcut: '⏎', label: commitLabel(active, allRows, inTrackedFile), run: commitEdit }];
      if (requiresProtectedInput(current, active.key)) {
        list.push({ shortcut: '⌥', label: 'hold to show', run: () => {}, hold: true });
      }
      list.push({ shortcut: 'esc', label: 'cancel', run: cancelEdit });
      return list;
    }
    if (path === NEW_ENV_TAB && example) {
      return [{ shortcut: '⏎', label: `create .env from ${example.name}`, run: () => void primaryAction() }];
    }
    if (current.files.length === 0 && current.incomplete.length === 0) {
      return [
        { shortcut: '⏎', label: 'create .env', run: () => void primaryAction() },
        { shortcut: '⌘O', label: 'choose another folder', run: browse }
      ];
    }
    if (!row) {
      return [];
    }
    if (row.kind === 'add') {
      return [{ shortcut: '⏎', label: 'add', run: () => void primaryAction() }];
    }
    const list: Verb[] = [{ shortcut: '⏎', label: 'edit', run: () => void primaryAction() }];
    if (row.need === null) {
      if (row.kind === 'entry' && row.entry.shape.redactedByDefault) {
        list.push({ shortcut: '⌥', label: 'hold to show', run: () => {}, hold: true });
      }
      list.push({ shortcut: '⌘C', label: 'copy', run: () => void copySelected() });
    }
    list.push({ shortcut: '⌘N', label: 'new key', run: () => void startAdd() });
    return list;
  }

  function commitLabel(active: Edit, allRows: Row[], inTrackedFile: boolean): string {
    const writes = active.rowId === ADD_ROW_ID || (active.value !== '' && active.value !== active.baseline);
    const action = writes ? (inTrackedFile ? 'save anyway' : 'save') : active.needed ? 'skip' : 'keep';
    if (!active.needed) {
      return action;
    }
    const next = nextNeedingIndex(allRows, allRows.findIndex((row) => row.id === active.rowId));
    return next === null ? action : `${action} · next: ${allRows[next].key}`;
  }

  function hintFor(
    current: ProjectSnapshot | null,
    file: EnvFile | null,
    row: Row | null,
    active: Edit | null,
    revealed: RevealedEntry | null
  ): { tone: NoteTone; text: string } | null {
    if (!current || !file || !row || active || row.kind !== 'entry') {
      return null;
    }
    if (isRevealed(row, revealed, file)) {
      return { tone: 'quiet', text: 'let go, or leave the window, and it hides' };
    }
    const duplicate = duplicateHint(current.root, file, row.entry);
    return duplicate ? { tone: 'danger', text: duplicate } : null;
  }

  function beginOperation(kind: OperationKind, target: EntryRef | null = null): PendingOperation | null {
    if (pendingOperation) {
      return null;
    }
    const operation = createPendingOperation(++operationGeneration, kind, target);
    pendingOperation = operation;
    return operation;
  }

  function settleOperation(operation: PendingOperation): boolean {
    if (!isCurrentOperation(pendingOperation, operation)) {
      return false;
    }
    pendingOperation = null;
    return true;
  }

  function currentSelection(): Selection | null {
    return selectedPath ? { path: selectedPath, rowId: selectedRowId } : null;
  }

  function applySnapshot(loaded: ProjectSnapshot, keep: Selection | null): void {
    snapshot = loaded;
    projectPath = loaded.root;
    revealed = null;
    const example = findExampleToCopy(loaded);

    if (keep) {
      if (keep.path === NEW_ENV_TAB && example) {
        selectedPath = NEW_ENV_TAB;
        selectedRowId = '';
        return;
      }
      if (keep.path.startsWith(UNREADABLE_TAB) && loaded.incomplete.some((item) => UNREADABLE_TAB + item.name === keep.path)) {
        selectedPath = keep.path;
        selectedRowId = '';
        return;
      }
      const file = loaded.files.find((candidate) => candidate.path === keep.path);
      if (file) {
        const fileRows = buildRows(loaded, file);
        selectedPath = file.path;
        selectedRowId = fileRows.some((row) => row.id === keep.rowId) ? keep.rowId : fileRows[0].id;
        return;
      }
    }

    if (example && !loaded.files.some((file) => !isExampleFile(file))) {
      selectedPath = NEW_ENV_TAB;
      selectedRowId = '';
      return;
    }
    const initial = initialSelection(loaded);
    selectedPath = initial?.path ?? (loaded.incomplete[0] ? UNREADABLE_TAB + loaded.incomplete[0].name : '');
    selectedRowId = initial?.rowId ?? '';
  }

  async function loadProjectPath(path: string, keep: Selection | null, quiet = false): Promise<void> {
    if (!path) {
      return;
    }
    const operation = beginOperation('load-project');
    if (!operation) {
      return;
    }
    loading = !quiet;
    try {
      const loaded = await loadProject(path);
      if (!settleOperation(operation)) {
        return;
      }
      loadError = '';
      applySnapshot(loaded, keep);
      retargetEdit();
    } catch (error) {
      if (settleOperation(operation)) {
        if (snapshot) {
          message = { tone: 'danger', text: `! could not reload: ${errorMessage(error)}` };
        } else {
          loadError = errorMessage(error);
        }
      }
    } finally {
      loading = false;
    }
  }

  async function bootProject(): Promise<void> {
    try {
      const initialPath = await initialProjectPath();
      await loadProjectPath(isTauriRuntime() ? initialPath ?? '' : initialPath ?? '.', null);
    } catch (error) {
      loadError = errorMessage(error);
    }
  }

  function reload(): void {
    void loadProjectPath(snapshot?.root ?? projectPath, currentSelection());
  }

  async function browse(): Promise<void> {
    if (edit || pendingOperation) {
      return;
    }
    if (!isTauriRuntime()) {
      message = { tone: 'warning', text: 'Choosing a folder is available in the desktop app.' };
      return;
    }
    const operation = beginOperation('browse-project');
    if (!operation) {
      return;
    }
    let picked: string | null = null;
    try {
      picked = await pickProjectDirectory(snapshot?.root ?? projectPath);
    } catch (error) {
      message = { tone: 'danger', text: `! ${errorMessage(error)}` };
    }
    if (!settleOperation(operation) || !picked) {
      return;
    }
    snapshot = null;
    selectedPath = '';
    selectedRowId = '';
    message = null;
    await loadProjectPath(picked, null);
  }

  /** Keeps what was typed across a reload by finding the same key again. */
  function retargetEdit(): void {
    if (!edit || !snapshot) {
      return;
    }
    const active = edit;
    const file = snapshot.files.find((candidate) => candidate.path === active.filePath);
    if (!file) {
      edit = null;
      message = { tone: 'danger', text: `! ${baseName(active.filePath)} is no longer on disk.` };
      return;
    }
    const fileRows = buildRows(snapshot, file);
    const row =
      active.rowId === ADD_ROW_ID
        ? fileRows.find((candidate) => candidate.id === ADD_ROW_ID)
        : (fileRows.find(
            (candidate) =>
              candidate.kind === 'entry' && candidate.key === active.key && candidate.entry.lineNumber === active.target?.lineNumber
          ) ?? fileRows.find((candidate) => candidate.key === active.key));
    if (!row) {
      edit = null;
      message = { tone: 'danger', text: `! ${active.key} is no longer in ${file.name}.` };
      return;
    }
    selectedPath = file.path;
    selectedRowId = row.id;
    edit = {
      ...active,
      rowId: row.id,
      target: row.kind === 'entry' ? createEntryRef(snapshot.root, file, row.entry) : null,
      needed: row.need !== null,
      refused: false
    };
    message = null;
    void focusEditInput();
  }

  function chooseTab(id: string): void {
    if (edit || pendingOperation || id === selectedPath) {
      return;
    }
    revealed = null;
    message = null;
    savedRowId = '';
    selectedPath = id;
    const file = snapshot?.files.find((candidate) => candidate.path === id);
    const fileRows = snapshot && file ? buildRows(snapshot, file) : [];
    selectedRowId = (fileRows.find((row) => row.need !== null) ?? fileRows[0])?.id ?? '';
  }

  function select(rowId: string): void {
    if (edit || pendingOperation || rowId === selectedRowId) {
      return;
    }
    revealed = null;
    message = null;
    savedRowId = '';
    selectedRowId = rowId;
  }

  function moveSelection(delta: number): void {
    if (rows.length === 0) {
      return;
    }
    // Read the id, not the reactive index, which can lag a selection set
    // earlier in the same task.
    const from = Math.max(0, rows.findIndex((row) => row.id === selectedRowId));
    select(rows[Math.min(Math.max(from + delta, 0), rows.length - 1)].id);
  }

  async function scrollSelectedIntoView(): Promise<void> {
    await tick();
    buffer?.querySelector('tr.sel')?.scrollIntoView({ block: 'nearest' });
  }

  async function focusEditInput(): Promise<void> {
    await tick();
    const input = edit?.phase === 'name' ? nameInput : valueInput;
    input?.focus({ preventScroll: true });
  }

  /** Waits out a focus-regain reload. False when the reload moved the
   * selection, so a key pressed before it never acts on a different row. */
  async function afterQuietLoad(): Promise<boolean> {
    if (!quietLoad) {
      return true;
    }
    const path = selectedPath;
    const rowId = selectedRowId;
    await quietLoad;
    await tick();
    return selectedPath === path && selectedRowId === rowId;
  }

  async function primaryAction(): Promise<void> {
    if (!(await afterQuietLoad())) {
      return;
    }
    if (!snapshot || edit || pendingOperation) {
      return;
    }
    if (selectedPath === NEW_ENV_TAB) {
      void createFromExample();
      return;
    }
    if (snapshot.files.length === 0 && snapshot.incomplete.length === 0) {
      void createEmptyEnv();
      return;
    }
    if (selectedFile && selectedRow) {
      void startEdit(selectedFile, selectedRow);
    }
  }

  async function startEdit(file: EnvFile, row: Row): Promise<void> {
    if (!snapshot || edit || pendingOperation) {
      return;
    }
    revealed = null;
    message = null;
    savedRowId = '';
    selectedPath = file.path;
    selectedRowId = row.id;
    if (row.kind === 'add') {
      edit = {
        rowId: row.id,
        filePath: file.path,
        target: null,
        phase: 'name',
        key: '',
        value: '',
        baseline: '',
        lines: 1,
        needed: false,
        takenRowId: '',
        refused: false
      };
    } else {
      const entry = row.kind === 'entry' ? row.entry : null;
      // A secret's stored value never reaches the window, so its field starts empty.
      const plain = entry && !entry.shape.redactedByDefault && entry.valueState === 'set' ? entry.value : '';
      edit = {
        rowId: row.id,
        filePath: file.path,
        target: entry ? createEntryRef(snapshot.root, file, entry) : null,
        phase: 'value',
        key: row.key,
        value: plain,
        baseline: plain,
        lines: plain.split('\n').length,
        needed: row.need !== null,
        takenRowId: '',
        refused: false
      };
    }
    await focusEditInput();
  }

  async function startAdd(): Promise<void> {
    if (!(await afterQuietLoad())) {
      return;
    }
    if (!selectedFile || edit || pendingOperation) {
      return;
    }
    const addRow = rows.find((row) => row.id === ADD_ROW_ID);
    if (addRow) {
      await startEdit(selectedFile, addRow);
    }
  }

  function cancelEdit(): void {
    if (pendingOperation) {
      return;
    }
    edit = null;
    message = null;
  }

  async function commitEdit(): Promise<void> {
    if (!edit || !snapshot || pendingOperation) {
      return;
    }
    const active = edit;
    const file = snapshot.files.find((candidate) => candidate.path === active.filePath);
    if (!file) {
      return;
    }

    if (active.takenRowId) {
      const row = buildRows(snapshot, file).find((candidate) => candidate.id === active.takenRowId);
      edit = null;
      if (row) {
        await startEdit(file, row);
      }
      return;
    }

    if (active.phase === 'name') {
      const name = active.key.trim();
      if (!name) {
        return;
      }
      if (name.includes('=')) {
        // The part after `=` may be a secret, so it is not repeated back.
        message = { tone: 'danger', text: `! ${name.split('=')[0]}=… looks like KEY=VALUE. Type the name alone, then the value.` };
        return;
      }
      if (!VALID_KEY_NAME.test(name)) {
        message = { tone: 'danger', text: `! ${name} is not a valid key name. Start with a letter or _, then letters, digits, _ . or -.` };
        return;
      }
      const taken = file.entries.filter((entry) => entry.key === name);
      if (taken.length > 0) {
        edit = { ...active, key: name, takenRowId: taken[0].id };
        message = { tone: 'danger', text: `! ${name} already exists at line ${taken.map((entry) => entry.lineNumber).join(', ')}.` };
        return;
      }
      edit = { ...active, key: name, phase: 'value' };
      message = null;
      await focusEditInput();
      return;
    }

    if (active.rowId === ADD_ROW_ID && active.value === '') {
      message = { tone: 'warning', text: `${active.key} needs a value before it can be added.` };
      return;
    }
    if (active.value === '' || active.value === active.baseline) {
      await openNextNeeding(file, buildRows(snapshot, file), active);
      return;
    }

    const root = snapshot.root;
    const operation = beginOperation(active.target ? 'save-entry' : 'add-entry', active.target);
    if (!operation) {
      return;
    }
    try {
      const loaded = active.target
        ? await saveEnvValue(root, active.filePath, active.key, active.target.lineNumber, active.value)
        : await addEnvKey(root, active.filePath, active.key, active.value);
      if (!settleOperation(operation)) {
        return;
      }
      snapshot = loaded;
      revealed = null;
      const savedFile = loaded.files.find((candidate) => candidate.path === active.filePath);
      const savedRows = savedFile ? buildRows(loaded, savedFile) : [];
      const saved = active.target
        ? savedRows.find(
            (row) => row.kind === 'entry' && row.key === active.key && row.entry.lineNumber === active.target?.lineNumber
          )
        : [...savedRows].reverse().find((row) => row.kind === 'entry' && row.key === active.key);
      edit = null;
      selectedPath = active.filePath;
      selectedRowId = saved?.id ?? selectedRowId;
      savedRowId = saved?.id ?? '';
      message = { tone: 'ok', text: `✓ saved ${active.key}` };
      if (savedFile && saved) {
        await openNextNeeding(savedFile, savedRows, { ...active, rowId: saved.id }, true);
      }
    } catch (error) {
      if (settleOperation(operation)) {
        edit = { ...active, refused: true };
        message = { tone: 'danger', text: refusalText(active, errorMessage(error)) };
      }
    }
  }

  /** After a row needing a value is saved or skipped, the next one opens. */
  async function openNextNeeding(file: EnvFile, fileRows: Row[], active: Edit, keepMessage = false): Promise<void> {
    edit = null;
    if (!keepMessage) {
      message = null;
    }
    if (!active.needed) {
      return;
    }
    const next = nextNeedingIndex(fileRows, fileRows.findIndex((row) => row.id === active.rowId));
    if (next !== null) {
      await startEdit(file, fileRows[next]);
      if (keepMessage) {
        message = { tone: 'ok', text: `✓ saved ${active.key}` };
      }
    }
  }

  function refusalText(active: Edit, error: string): string {
    if (active.target && error.endsWith('was not found')) {
      return `! ${active.key} is no longer on line ${active.target.lineNumber}; ${baseName(active.filePath)} changed on disk. Nothing saved.`;
    }
    return `! ${error} Nothing saved.`;
  }

  async function createFromExample(): Promise<void> {
    const example = exampleToCopy;
    if (!example) {
      return;
    }
    const operation = beginOperation('create-from-example');
    if (!operation) {
      return;
    }
    try {
      const loaded = await createEnvFromExample();
      if (!settleOperation(operation)) {
        return;
      }
      applySnapshot(loaded, null);
      message = { tone: 'ok', text: `✓ created .env from ${example.name}` };
    } catch (error) {
      if (settleOperation(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  async function createEmptyEnv(): Promise<void> {
    if (!snapshot) {
      return;
    }
    const root = snapshot.root;
    const operation = beginOperation('create-file');
    if (!operation) {
      return;
    }
    try {
      const outcome = await ensureEnvFile(root, '.env');
      if (!settleOperation(operation)) {
        return;
      }
      await loadProjectPath(root, { path: outcome.path, rowId: ADD_ROW_ID });
      message = { tone: 'ok', text: '✓ created .env' };
    } catch (error) {
      if (settleOperation(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  async function startReveal(): Promise<void> {
    if (optionHeld) {
      return;
    }
    optionHeld = true;
    if (!(await afterQuietLoad())) {
      return;
    }
    const row = selectedRow;
    const target = selectedTarget;
    if (edit || !target || row?.kind !== 'entry' || !row.entry.shape.redactedByDefault || row.entry.valueState === 'empty') {
      return;
    }
    const operation = beginOperation('reveal-entry', target);
    if (!operation) {
      return;
    }
    try {
      const value = await revealEnvValue(target.root, target.filePath, target.key, target.lineNumber);
      const current = canApplyTargetedOperation(pendingOperation, operation, selectedTarget);
      settleOperation(operation);
      if (current && optionHeld) {
        revealed = { target, value };
      }
    } catch (error) {
      if (settleOperation(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  function endReveal(): void {
    optionHeld = false;
    revealed = null;
  }

  async function copySelected(): Promise<void> {
    if (!(await afterQuietLoad())) {
      return;
    }
    if (edit || pendingOperation || !selectedRow || selectedRow.kind === 'add') {
      return;
    }
    if (selectedRow.need !== null) {
      message = { tone: 'warning', text: `${selectedRow.key} has no value yet, so there is nothing to copy.` };
      return;
    }
    const target = selectedTarget;
    if (!target) {
      return;
    }
    const operation = beginOperation('copy-entry', target);
    if (!operation) {
      return;
    }
    try {
      const outcome = await copyEnvValue(target.root, target.filePath, target.key, target.lineNumber);
      if (!settleOperation(operation)) {
        return;
      }
      message = null;
      if (outcome.copyId !== null && outcome.clearsInSeconds !== null) {
        now = Date.now();
        pendingClear = { key: target.key, copyId: outcome.copyId, endsAt: now + outcome.clearsInSeconds * 1000 };
      } else {
        pendingClear = null;
        message = { tone: 'ok', text: `✓ copied ${target.key}` };
      }
    } catch (error) {
      if (settleOperation(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === 'Alt') {
      void startReveal();
      return;
    }
    const command = event.metaKey || event.ctrlKey;
    const key = event.key.toLowerCase();
    if (command && key === 'r') {
      event.preventDefault();
      reload();
      return;
    }
    const typing = event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement;
    if (edit && !typing) {
      // The field lost focus, for example to a click; its keys still apply.
      if (event.key === 'Enter') {
        event.preventDefault();
        void commitEdit();
      } else if (event.key === 'Escape') {
        event.preventDefault();
        cancelEdit();
      }
      return;
    }
    if (typing || !snapshot) {
      if (!snapshot && command && key === 'o') {
        event.preventDefault();
        void browse();
      }
      return;
    }
    if (command) {
      if (key === 'o') {
        event.preventDefault();
        void browse();
      } else if (/^[1-9]$/.test(event.key) && tabs[Number(event.key) - 1]) {
        event.preventDefault();
        chooseTab(tabs[Number(event.key) - 1].id);
      } else if (key === 'c' && !window.getSelection()?.toString()) {
        event.preventDefault();
        void copySelected();
      } else if (key === 'n') {
        event.preventDefault();
        void startAdd();
      }
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      void afterQuietLoad().then((unmoved) => unmoved && moveSelection(delta));
    } else if (event.key === 'Enter' && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      void primaryAction();
    }
  }

  function handleWindowKeyup(event: KeyboardEvent): void {
    if (event.key === 'Alt') {
      endReveal();
    }
  }

  function handleWindowFocus(): void {
    // Picks up changes made in a terminal while the window was in the background.
    if (snapshot && !edit && !pendingOperation) {
      quietLoad = loadProjectPath(snapshot.root, currentSelection(), true).finally(() => {
        quietLoad = null;
      });
    }
  }

  function handleEditKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      void commitEdit();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      cancelEdit();
    }
  }

  function handleValueInput(value: string): void {
    if (edit && !pendingOperation) {
      edit = { ...edit, value };
    }
  }

  /** An input drops line breaks, so a multi-line paste is kept whole. */
  function handleValuePaste(event: ClipboardEvent): void {
    const text = event.clipboardData?.getData('text') ?? '';
    if (!edit || !text.includes('\n')) {
      return;
    }
    event.preventDefault();
    edit = { ...edit, value: text, lines: text.split('\n').length };
  }

  function valueText(row: Row, revealed: RevealedEntry | null, file: EnvFile | null): string {
    if (row.kind !== 'entry') {
      return '';
    }
    return isRevealed(row, revealed, file) && revealed ? revealed.value : entryDisplay(row.entry);
  }

  /** What a row shows without a reveal: a secret is masked, an empty value blank. */
  function entryDisplay(entry: EnvEntry): string {
    if (entry.valueState === 'empty') {
      return '';
    }
    return entry.shape.redactedByDefault ? '••••••••' : entry.value;
  }

  function isRevealed(row: Row, revealed: RevealedEntry | null, file: EnvFile | null): boolean {
    return Boolean(
      revealed && file && row.kind === 'entry' && revealed.target.filePath === file.path && revealed.target.entryId === row.entry.id
    );
  }

  function noteFor(
    row: Row,
    active: Edit | null,
    revealed: RevealedEntry | null,
    file: EnvFile | null,
    saved: string
  ): { tone: NoteTone | MessageTone; text: string } | null {
    if (saved === row.id) {
      return { tone: 'ok', text: '✓ saved' };
    }
    if (isRevealed(row, revealed, file)) {
      return { tone: 'info', text: 'shown while ⌥ is held' };
    }
    if (active?.rowId === row.id && active.lines > 1) {
      return { tone: 'info', text: `${active.lines} lines` };
    }
    return row.note;
  }

  function placeholderFor(row: Row, active: Edit): string {
    if (active.lines > 1) {
      return '';
    }
    if (row.kind === 'entry' && row.need === null && row.entry.shape.redactedByDefault) {
      return 'type a new value · ⏎ on empty keeps the old one';
    }
    return row.need !== null ? 'type the value · ⏎ on empty skips' : 'type the value';
  }

  function lineLabel(row: Row | null): string {
    return row?.kind === 'entry' ? String(row.entry.lineNumber) : '+';
  }

  function keyLabel(row: Row | null, active: Edit | null): string {
    if (row?.kind === 'add') {
      return active?.key || 'new key';
    }
    return row?.key ?? '';
  }

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  onMount(() => {
    void bootProject();
    const timer = window.setInterval(() => {
      if (pendingClear) {
        now = Date.now();
      }
    }, 250);
    const unlisten = onConcealedCopyExpired((event) => {
      // An earlier copy's timer must not end the countdown of a later one.
      if (event.copyId !== pendingClear?.copyId) {
        return;
      }
      pendingClear = null;
      if (!edit) {
        message = event.cleared
          ? { tone: 'info', text: 'clipboard cleared' }
          : { tone: 'info', text: 'clipboard left alone: something else was copied after it' };
      }
    });
    return () => {
      window.clearInterval(timer);
      void unlisten.then((stop) => stop());
    };
  });
</script>

<svelte:head>
  <title>vne</title>
</svelte:head>

<svelte:window onkeydown={handleWindowKeydown} onkeyup={handleWindowKeyup} onblur={endReveal} onfocus={handleWindowFocus} />

<main class="app" aria-busy={isBusy}>
  <nav class="tabline" aria-label="Env files">
    {#each tabs as tab, index (tab.id)}
      <button
        type="button"
        class="tb"
        class:on={tab.id === selectedPath}
        class:ghost={tab.ghost}
        aria-current={tab.id === selectedPath ? 'page' : undefined}
        title={index < 9 ? `⌘${index + 1}` : undefined}
        onclick={() => chooseTab(tab.id)}
      >
        {tab.label}{#if tab.count !== null}<span class="n">{tab.count}</span>{/if}{#if tab.badge}<span class="badge {tab.badgeTone}">{tab.badge}</span>{/if}
      </button>
    {/each}
  </nav>

  <section class="buf" bind:this={buffer} aria-label="Keys">
    {#if !snapshot}
      <p class="lead">
        {#if loading}
          scanning {projectPath || 'the project'}…
        {:else if loadError}
          <span class="danger">! could not open {projectPath || 'the folder'}: {loadError}</span>
        {:else}
          No folder open.
        {/if}
      </p>
    {:else if selectedPath === NEW_ENV_TAB && exampleToCopy}
      <p class="lead">
        No <b>.env</b> yet. ⏎ creates it as a copy of <b>{exampleToCopy.name}</b> ({exampleToCopy.entries.length} keys).<br />
        Then <span class="warning">{newEnvNeeds} need a value</span>{#if newEnvNeeds < exampleToCopy.entries.length}; the rest keep the example's values{/if}.
      </p>
      <table class="rows preview">
        <tbody>
          {#each exampleToCopy.entries as entry (entry.id)}
            <tr>
              <td class="n">{entry.lineNumber}</td>
              <td class="k" title={entry.key}>{entry.key}</td>
              <td class="val" class:masked={entry.shape.redactedByDefault}>
                {entryDisplay(entry)}
              </td>
              <td class="vt warning">{entry.valueState === 'set' ? '' : entry.valueState}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else if unreadable}
      <p class="lead">
        vne could not read <b>{unreadable.name}</b> <span class="warning">({unreadable.reason})</span>.<br />
        Nothing in it can be shown, copied or edited here.
      </p>
    {:else if snapshot.files.length === 0}
      <p class="lead">No env files in <b>{snapshot.root}</b>.</p>
    {:else if selectedFile}
      <table class="rows">
        <tbody aria-label={`Keys in ${selectedFile.name}`}>
          {#each rows as row (row.id)}
            {@const editing = edit !== null && edit.rowId === row.id}
            {@const note = noteFor(row, edit, revealed, selectedFile, savedRowId)}
            <tr
              class:sel={row.id === selectedRowId}
              class:ghost={row.kind === 'missing'}
              class:add={row.kind === 'add'}
              class:wrap={isRevealed(row, revealed, selectedFile)}
              aria-current={row.id === selectedRowId ? 'true' : undefined}
              onclick={() => select(row.id)}
              ondblclick={() => selectedFile && void startEdit(selectedFile, row)}
            >
              <td class="n">{lineLabel(row)}</td>
              <td class="k" title={row.key || undefined}>
                {#if editing && edit && edit.phase === 'name'}
                  <input
                    bind:this={nameInput}
                    value={edit.key}
                    aria-label="New key name"
                    placeholder="name, then ⏎ for the value"
                    autocomplete="off"
                    autocapitalize="off"
                    spellcheck="false"
                    readonly={isBusy}
                    oninput={(event) => edit && (edit = { ...edit, key: event.currentTarget.value, takenRowId: '' })}
                    onkeydown={handleEditKeydown}
                  />
                {:else if row.kind === 'add'}
                  {editing && edit ? edit.key : 'add a key'}
                {:else}
                  {row.key}
                {/if}
              </td>
              <td class="val" class:masked={row.kind === 'entry' && row.entry.shape.redactedByDefault && !isRevealed(row, revealed, selectedFile)}>
                {#if editing && edit && edit.phase === 'value'}
                  <input
                    bind:this={valueInput}
                    type={inputTypeForKey(snapshot, edit.key, optionHeld)}
                    value={edit.lines > 1 ? edit.value.replaceAll('\n', ' ↵ ') : edit.value}
                    aria-label={`Value for ${edit.key}`}
                    placeholder={placeholderFor(row, edit)}
                    autocomplete="off"
                    spellcheck="false"
                    readonly={isBusy || edit.lines > 1}
                    oninput={(event) => handleValueInput(event.currentTarget.value)}
                    onpaste={handleValuePaste}
                    onkeydown={handleEditKeydown}
                  />
                {:else}
                  {valueText(row, revealed, selectedFile)}
                {/if}
              </td>
              <td class="vt {note?.tone ?? ''}">{note?.text ?? ''}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>

  <div class="status">
    <span class="mode" class:edit={edit !== null}>{edit ? 'edit' : 'view'}</span>
    <span class="seg"><b>{selectedPath === NEW_ENV_TAB ? '.env (not created yet)' : selectedFile?.name ?? unreadable?.name ?? snapshot?.root ?? ''}</b></span>
    {#if selectedRow}
      <span class="seg">ln {lineLabel(selectedRow)}</span>
      <span class="seg"><b>{keyLabel(selectedRow, edit)}</b></span>
    {/if}
    <span class="sp"></span>
    {#if needs > 0}<span class="rv">{needs} need a value</span>{/if}
  </div>

  <!-- Always present, so screen readers announce changes; the copy countdown stays out of it. -->
  <p class="sr-only" role="status" aria-live="polite">{message?.text ?? ''}</p>

  <div class="cmd">
    {#if promptMessage}
      <span class="msg {promptMessage.tone}">{promptMessage.text}</span>
    {:else if edit && tracked && edit.phase === 'value'}
      <span class="msg warning">! git tracks {selectedFile?.name}; a commit would publish this value.</span>
    {:else if !edit}
      <span class="p">›</span>
    {/if}
    {#each verbs as verb (verb.shortcut + verb.label)}
      {#if verb.hold}
        <button
          type="button"
          class="verb"
          onpointerdown={(event) => event.button === 0 && void startReveal()}
          onpointerup={endReveal}
          onpointerleave={endReveal}
          onpointercancel={endReveal}
        ><b>{verb.shortcut}</b>{verb.label}</button>
      {:else}
        <button type="button" class="verb" disabled={isBusy} onclick={verb.run}><b>{verb.shortcut}</b>{verb.label}</button>
      {/if}
    {/each}
    {#if hint}<span class="hint {hint.tone}">{hint.text}</span>{/if}
  </div>
</main>
