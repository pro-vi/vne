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
    onClipboardCleared,
    pickProjectDirectory,
    revealEnvValue,
    saveEnvValue
  } from './lib/tauri';
  import type { EnvFile, ProjectSnapshot } from './lib/types';
  import {
    ADD_ROW_ID,
    buildRows,
    duplicateHint,
    freshCloneExample,
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

  type Tone = 'ok' | 'warning' | 'danger' | 'info';
  type Message = { tone: Tone; text: string };
  type Verb = { keys: string; label: string; run: () => void; hold?: boolean };
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

  const FRESH_TAB = '\u0000fresh';
  const UNREAD_TAB = '\u0000unread:';
  /** Same rule as `is_valid_env_key` in src-tauri/src/lib.rs. */
  const KEY_NAME = /^[A-Za-z_][A-Za-z0-9_.-]*$/;

  let projectPath = '';
  let snapshot: ProjectSnapshot | null = null;
  let loading = false;
  let loadError = '';
  let selectedPath = '';
  let selectedRowId = '';
  let edit: Edit | null = null;
  let shown: RevealedEntry | null = null;
  let optionHeld = false;
  let message: Message | null = null;
  let savedRowId = '';
  let copyClock: { key: string; endsAt: number } | null = null;
  let now = Date.now();
  let pendingOperation: PendingOperation | null = null;
  let operationGeneration = 0;
  let nameInput: HTMLInputElement | null = null;
  let valueInput: HTMLInputElement | null = null;
  let buffer: HTMLElement | null = null;

  $: fresh = snapshot ? freshCloneExample(snapshot) : null;
  $: tabs = buildTabs(snapshot, fresh);
  $: selectedFile = snapshot?.files.find((file) => file.path === selectedPath) ?? null;
  $: rows = snapshot && selectedFile ? buildRows(snapshot, selectedFile) : [];
  $: selectedIndex = rows.findIndex((row) => row.id === selectedRowId);
  $: selectedRow = selectedIndex >= 0 ? rows[selectedIndex] : null;
  $: selectedTarget = targetFor(snapshot, selectedFile, selectedRow);
  $: needs = needCount(rows);
  $: tracked = selectedFile?.gitStatus === 'tracked';
  $: isBusy = pendingOperation !== null;
  $: unread = selectedPath.startsWith(UNREAD_TAB)
    ? snapshot?.incomplete.find((item) => UNREAD_TAB + item.name === selectedPath) ?? null
    : null;
  $: freshNeeds = fresh ? fresh.entries.filter((entry) => entry.valueState !== 'set').length : 0;
  $: copySeconds = copyClock ? Math.max(0, Math.ceil((copyClock.endsAt - now) / 1000)) : 0;
  $: promptMessage = message ?? (copyClock ? { tone: 'ok' as Tone, text: `✓ copied ${copyClock.key} · clipboard clears in ${copySeconds} s` } : null);
  $: verbs = verbsFor(snapshot, selectedPath, selectedRow, rows, edit, tracked, fresh, loadError);
  $: hint = hintFor(snapshot, selectedFile, selectedRow, edit, shown);
  $: if (selectedRowId && buffer) void scrollSelectedIntoView();

  function buildTabs(current: ProjectSnapshot | null, example: EnvFile | null): Tab[] {
    if (!current) {
      return [];
    }
    const freshTab: Tab[] = example
      ? [{ id: FRESH_TAB, label: '.env', count: null, badge: 'new', badgeTone: 'warning', ghost: true }]
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
      id: UNREAD_TAB + item.name,
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
        ? [{ keys: '⌘O', label: 'choose folder', run: browse }, { keys: '⌘R', label: 'try again', run: reload }]
        : [{ keys: '⌘O', label: 'choose folder', run: browse }];
    }
    if (active) {
      if (active.refused) {
        return [
          { keys: '⌘R', label: 'reload, keep what you typed', run: reload },
          { keys: 'esc', label: 'cancel', run: cancelEdit }
        ];
      }
      if (active.takenRowId) {
        const line = allRows.find((candidate) => candidate.id === active.takenRowId);
        const lineNumber = line?.kind === 'entry' ? line.entry.lineNumber : '';
        return [
          { keys: '⏎', label: `edit line ${lineNumber}`, run: commitEdit },
          { keys: 'esc', label: 'cancel', run: cancelEdit }
        ];
      }
      if (active.phase === 'name') {
        return [
          { keys: '⏎', label: 'next', run: commitEdit },
          { keys: 'esc', label: 'cancel', run: cancelEdit }
        ];
      }
      const list: Verb[] = [{ keys: '⏎', label: commitLabel(active, allRows, inTrackedFile), run: commitEdit }];
      if (requiresProtectedInput(current, active.key)) {
        list.push({ keys: '⌥', label: 'hold to show', run: () => {}, hold: true });
      }
      list.push({ keys: 'esc', label: 'cancel', run: cancelEdit });
      return list;
    }
    if (path === FRESH_TAB && example) {
      return [{ keys: '⏎', label: `create .env from ${example.name}`, run: primaryAction }];
    }
    if (current.files.length === 0 && current.incomplete.length === 0) {
      return [
        { keys: '⏎', label: 'create .env', run: primaryAction },
        { keys: '⌘O', label: 'choose another folder', run: browse }
      ];
    }
    if (!row) {
      return [];
    }
    if (row.kind === 'add') {
      return [{ keys: '⏎', label: 'add', run: primaryAction }];
    }
    const list: Verb[] = [{ keys: '⏎', label: 'edit', run: primaryAction }];
    if (row.need === null) {
      if (row.kind === 'entry' && row.entry.shape.redactedByDefault) {
        list.push({ keys: '⌥', label: 'hold to show', run: () => {}, hold: true });
      }
      list.push({ keys: '⌘C', label: 'copy', run: () => void copySelected() });
    }
    list.push({ keys: '⌘N', label: 'new key', run: () => void startAdd() });
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
    if (revealed && revealed.target.entryId === row.entry.id && revealed.target.filePath === file.path) {
      return { tone: 'quiet', text: 'let go, or leave the window, and it hides' };
    }
    const duplicate = duplicateHint(current.root, file, row.entry);
    return duplicate ? { tone: 'danger', text: duplicate } : null;
  }

  function begin(kind: OperationKind, target: EntryRef | null = null): PendingOperation | null {
    if (pendingOperation) {
      return null;
    }
    const operation = createPendingOperation(++operationGeneration, kind, target);
    pendingOperation = operation;
    return operation;
  }

  function settle(operation: PendingOperation): boolean {
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
    shown = null;
    const example = freshCloneExample(loaded);

    if (keep) {
      if (keep.path === FRESH_TAB && example) {
        selectedPath = FRESH_TAB;
        selectedRowId = '';
        return;
      }
      if (keep.path.startsWith(UNREAD_TAB) && loaded.incomplete.some((item) => UNREAD_TAB + item.name === keep.path)) {
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
      selectedPath = FRESH_TAB;
      selectedRowId = '';
      return;
    }
    const initial = initialSelection(loaded);
    selectedPath = initial?.path ?? (loaded.incomplete[0] ? UNREAD_TAB + loaded.incomplete[0].name : '');
    selectedRowId = initial?.rowId ?? '';
  }

  async function loadProjectPath(path: string, keep: Selection | null, quiet = false): Promise<void> {
    if (!path) {
      return;
    }
    const operation = begin('load-project');
    if (!operation) {
      return;
    }
    loading = !quiet;
    try {
      const loaded = await loadProject(path);
      if (!settle(operation)) {
        return;
      }
      loadError = '';
      applySnapshot(loaded, keep);
      retargetEdit();
    } catch (error) {
      if (settle(operation)) {
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
    if (edit || isBusy) {
      return;
    }
    if (!isTauriRuntime()) {
      message = { tone: 'warning', text: 'Choosing a folder is available in the desktop app.' };
      return;
    }
    const operation = begin('browse-project');
    if (!operation) {
      return;
    }
    let picked: string | null = null;
    try {
      picked = await pickProjectDirectory(snapshot?.root ?? projectPath);
    } catch (error) {
      message = { tone: 'danger', text: `! ${errorMessage(error)}` };
    }
    if (!settle(operation) || !picked) {
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
    if (edit || isBusy || id === selectedPath) {
      return;
    }
    shown = null;
    message = null;
    savedRowId = '';
    selectedPath = id;
    const file = snapshot?.files.find((candidate) => candidate.path === id);
    const fileRows = snapshot && file ? buildRows(snapshot, file) : [];
    selectedRowId = (fileRows.find((row) => row.need !== null) ?? fileRows[0])?.id ?? '';
  }

  function select(rowId: string): void {
    if (edit || isBusy || rowId === selectedRowId) {
      return;
    }
    shown = null;
    message = null;
    savedRowId = '';
    selectedRowId = rowId;
  }

  function move(delta: number): void {
    if (rows.length === 0) {
      return;
    }
    const from = selectedIndex < 0 ? 0 : selectedIndex;
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

  function primaryAction(): void {
    if (!snapshot || edit || isBusy) {
      return;
    }
    if (selectedPath === FRESH_TAB) {
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
    if (!snapshot || edit || isBusy) {
      return;
    }
    shown = null;
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
    if (!selectedFile || edit || isBusy) {
      return;
    }
    const addRow = rows.find((row) => row.id === ADD_ROW_ID);
    if (addRow) {
      await startEdit(selectedFile, addRow);
    }
  }

  function cancelEdit(): void {
    if (isBusy) {
      return;
    }
    edit = null;
    message = null;
  }

  async function commitEdit(): Promise<void> {
    if (!edit || !snapshot || isBusy) {
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
      if (!KEY_NAME.test(name)) {
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
      await moveOn(file, buildRows(snapshot, file), active);
      return;
    }

    const root = snapshot.root;
    const operation = begin(active.target ? 'save-entry' : 'add-entry', active.target);
    if (!operation) {
      return;
    }
    try {
      const loaded = active.target
        ? await saveEnvValue(root, active.filePath, active.key, active.target.lineNumber, active.value)
        : await addEnvKey(root, active.filePath, active.key, active.value);
      if (!settle(operation)) {
        return;
      }
      snapshot = loaded;
      shown = null;
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
        await moveOn(savedFile, savedRows, { ...active, rowId: saved.id }, true);
      }
    } catch (error) {
      if (settle(operation)) {
        edit = { ...active, refused: true };
        message = { tone: 'danger', text: refusalText(active, errorMessage(error)) };
      }
    }
  }

  /** After a row needing a value is saved or skipped, the next one opens. */
  async function moveOn(file: EnvFile, fileRows: Row[], active: Edit, keepMessage = false): Promise<void> {
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
    const example = fresh;
    const operation = begin('create-from-example');
    if (!operation || !example) {
      return;
    }
    try {
      const loaded = await createEnvFromExample();
      if (!settle(operation)) {
        return;
      }
      applySnapshot(loaded, null);
      message = { tone: 'ok', text: `✓ created .env from ${example.name}` };
    } catch (error) {
      if (settle(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  async function createEmptyEnv(): Promise<void> {
    if (!snapshot) {
      return;
    }
    const root = snapshot.root;
    const operation = begin('create-file');
    if (!operation) {
      return;
    }
    try {
      const outcome = await ensureEnvFile(root, '.env');
      if (!settle(operation)) {
        return;
      }
      await loadProjectPath(root, { path: outcome.path, rowId: ADD_ROW_ID });
      message = { tone: 'ok', text: '✓ created .env' };
    } catch (error) {
      if (settle(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  async function pressShow(): Promise<void> {
    if (optionHeld) {
      return;
    }
    optionHeld = true;
    const row = selectedRow;
    const target = selectedTarget;
    if (edit || !target || row?.kind !== 'entry' || !row.entry.shape.redactedByDefault || row.entry.valueState === 'empty') {
      return;
    }
    const operation = begin('reveal-entry', target);
    if (!operation) {
      return;
    }
    try {
      const value = await revealEnvValue(target.root, target.filePath, target.key, target.lineNumber);
      const current = canApplyTargetedOperation(pendingOperation, operation, selectedTarget);
      settle(operation);
      if (current && optionHeld) {
        shown = { target, value };
      }
    } catch (error) {
      if (settle(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  function releaseShow(): void {
    optionHeld = false;
    shown = null;
  }

  async function copySelected(): Promise<void> {
    if (edit || isBusy || !selectedRow || selectedRow.kind === 'add') {
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
    const operation = begin('copy-entry', target);
    if (!operation) {
      return;
    }
    try {
      const outcome = await copyEnvValue(target.root, target.filePath, target.key, target.lineNumber);
      if (!settle(operation)) {
        return;
      }
      message = null;
      if (outcome.clearsInSeconds) {
        now = Date.now();
        copyClock = { key: outcome.key, endsAt: now + outcome.clearsInSeconds * 1000 };
      } else {
        copyClock = null;
        message = { tone: 'ok', text: `✓ copied ${outcome.key}` };
      }
    } catch (error) {
      if (settle(operation)) {
        message = { tone: 'danger', text: `! ${errorMessage(error)}` };
      }
    }
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.key === 'Alt') {
      void pressShow();
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
    if (typing || edit || !snapshot) {
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
      move(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      primaryAction();
    }
  }

  function handleWindowKeyup(event: KeyboardEvent): void {
    if (event.key === 'Alt') {
      releaseShow();
    }
  }

  function handleWindowFocus(): void {
    // Picks up changes made in a terminal while the window was in the background.
    if (snapshot && !edit && !isBusy) {
      void loadProjectPath(snapshot.root, currentSelection(), true);
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
    if (edit && !isBusy) {
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
    if (isShown(row, revealed, file) && revealed) {
      return revealed.value;
    }
    if (row.entry.valueState === 'empty') {
      return '';
    }
    return row.entry.shape.redactedByDefault ? '••••••••' : row.entry.value;
  }

  function isShown(row: Row, revealed: RevealedEntry | null, file: EnvFile | null): boolean {
    return Boolean(
      revealed && file && row.kind === 'entry' && revealed.target.filePath === file.path && revealed.target.entryId === row.entry.id
    );
  }

  function noteFor(row: Row, active: Edit | null, revealed: RevealedEntry | null, file: EnvFile | null, saved: string): { tone: string; text: string } | null {
    if (saved === row.id) {
      return { tone: 'ok', text: '✓ saved' };
    }
    if (isShown(row, revealed, file)) {
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

  function baseName(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  onMount(() => {
    void bootProject();
    const timer = window.setInterval(() => {
      if (copyClock) {
        now = Date.now();
      }
    }, 250);
    const unlisten = onClipboardCleared((event) => {
      copyClock = null;
      message = event.cleared
        ? { tone: 'info', text: 'clipboard cleared' }
        : { tone: 'info', text: 'clipboard left alone: something else was copied after it' };
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

<svelte:window onkeydown={handleWindowKeydown} onkeyup={handleWindowKeyup} onblur={releaseShow} onfocus={handleWindowFocus} />

<main class="app" aria-busy={isBusy}>
  <nav class="tabline" aria-label="Env files">
    {#each tabs as tab, index (tab.id)}
      <button
        type="button"
        class="tb"
        class:on={tab.id === selectedPath}
        class:ghost={tab.ghost}
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
    {:else if selectedPath === FRESH_TAB && fresh}
      <p class="lead">
        No <b>.env</b> yet. ⏎ creates it as a copy of <b>{fresh.name}</b> ({fresh.entries.length} keys).<br />
        Then <span class="warning">{freshNeeds} need a value</span>; the rest keep the example's values.
      </p>
      <table class="rows preview">
        <tbody>
          {#each fresh.entries as entry (entry.id)}
            <tr>
              <td class="n">{entry.lineNumber}</td>
              <td class="k" title={entry.key}>{entry.key}</td>
              <td class="val" class:masked={entry.shape.redactedByDefault}>
                {entry.valueState === 'empty' ? '' : entry.shape.redactedByDefault ? '••••••••' : entry.value}
              </td>
              <td class="vt warning">{entry.valueState === 'set' ? '' : entry.valueState}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else if unread}
      <p class="lead">
        vne could not read <b>{unread.name}</b> <span class="warning">({unread.reason})</span>.<br />
        Nothing in it can be shown, copied or edited here.
      </p>
    {:else if snapshot.files.length === 0}
      <p class="lead">No env files in <b>{snapshot.root}</b>.</p>
    {:else if selectedFile}
      <table class="rows">
        <tbody aria-label={`Keys in ${selectedFile.name}`}>
          {#each rows as row (row.id)}
            {@const editing = edit !== null && edit.rowId === row.id}
            {@const note = noteFor(row, edit, shown, selectedFile, savedRowId)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <tr
              class:sel={row.id === selectedRowId}
              class:ghost={row.kind === 'missing'}
              class:add={row.kind === 'add'}
              class:wrap={isShown(row, shown, selectedFile)}
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
              <td class="val" class:masked={row.kind === 'entry' && row.entry.shape.redactedByDefault && !isShown(row, shown, selectedFile)}>
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
                  {valueText(row, shown, selectedFile)}
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
    <span class="seg"><b>{selectedPath === FRESH_TAB ? '.env (not created yet)' : selectedFile?.name ?? unread?.name ?? snapshot?.root ?? ''}</b></span>
    {#if selectedRow}
      <span class="seg">ln {lineLabel(selectedRow)}</span>
      <span class="seg"><b>{keyLabel(selectedRow, edit)}</b></span>
    {/if}
    <span class="sp"></span>
    {#if needs > 0}<span class="rv">{needs} need a value</span>{/if}
  </div>

  <div class="cmd">
    {#if promptMessage}
      <span class="msg {promptMessage.tone}" role={promptMessage.tone === 'danger' ? 'alert' : 'status'}>{promptMessage.text}</span>
    {:else if edit && tracked && edit.phase === 'value'}
      <span class="msg warning">! git tracks {selectedFile?.name}; a commit would publish this value.</span>
    {:else if !edit}
      <span class="p">›</span>
    {/if}
    {#each verbs as verb (verb.keys + verb.label)}
      {#if verb.hold}
        <button
          type="button"
          class="verb"
          onpointerdown={() => void pressShow()}
          onpointerup={releaseShow}
          onpointerleave={releaseShow}
        ><b>{verb.keys}</b>{verb.label}</button>
      {:else}
        <button type="button" class="verb" disabled={isBusy} onclick={verb.run}><b>{verb.keys}</b>{verb.label}</button>
      {/if}
    {/each}
    {#if hint}<span class="hint {hint.tone}">{hint.text}</span>{/if}
  </div>
</main>
