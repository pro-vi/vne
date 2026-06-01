<script lang="ts">
  import {
    AlertTriangle,
    CheckCircle2,
    Eye,
    EyeOff,
    FolderOpen,
    FolderSearch,
    RefreshCw,
    Save,
    Search,
    ShieldCheck
  } from '@lucide/svelte';
  import { addEnvKey, isTauriRuntime, loadProject, pickProjectDirectory, saveEnvValue } from './lib/tauri';
  import type { EnvEntry, EnvFile, EnvRepairAction, FrameworkEnvProfile, ProjectSnapshot } from './lib/types';
  import { isEntryValueHidden, keyStatus, statusLabel, totalIssueCount } from './lib/summary';

  type Notice = {
    kind: 'idle' | 'loading' | 'success' | 'error';
    message: string;
  };

  let projectPath = '.';
  let snapshot: ProjectSnapshot | null = null;
  let selectedPath = '';
  let selectedKey = '';
  let editValue = '';
  let filter = '';
  let filterInput: HTMLInputElement | null = null;
  let showRaw = false;
  let showSecrets = false;
  let notice: Notice = {
    kind: 'idle',
    message: isTauriRuntime() ? 'Open a project directory to inspect env files.' : 'Browser preview is using local sample data.'
  };

  $: selectedFile = snapshot?.files.find((file) => file.path === selectedPath) ?? snapshot?.files[0] ?? null;
  $: selectedEntry = selectedFile?.entries.find((entry) => entry.key === selectedKey) ?? selectedFile?.entries[0] ?? null;
  $: visibleEntries = selectedFile ? filterEntries(selectedFile, filter) : [];
  $: selectedValueHidden = selectedEntry ? isEntryValueHidden(selectedEntry, showSecrets) : false;
  $: selectedFileHasHiddenSecrets = Boolean(selectedFile?.entries.some((entry) => isEntryValueHidden(entry, showSecrets)));
  $: issueCount = totalIssueCount(snapshot);
  $: canSave = Boolean(isTauriRuntime() && selectedFile && selectedEntry && !selectedValueHidden && editValue !== selectedEntry.value);
  $: layerConflicts = snapshot?.layerReport.overrides.filter((override) => override.conflict) ?? [];
  $: frameworkMissingCount = snapshot?.frameworkProfiles.reduce((total, profile) => total + profile.missingFiles.length, 0) ?? 0;
  $: repairActions = snapshot?.repairActions ?? [];
  $: if (selectedFileHasHiddenSecrets && showRaw) {
    showRaw = false;
  }

  async function openProject(): Promise<void> {
    notice = { kind: 'loading', message: 'Scanning env files...' };
    try {
      const loaded = await loadProject(projectPath.trim() || '.');
      snapshot = loaded;
      selectedPath = loaded.files[0]?.path ?? '';
      selectedKey = loaded.files[0]?.entries[0]?.key ?? '';
      editValue = loaded.files[0]?.entries[0]?.value ?? '';
      notice = {
        kind: 'success',
        message: loaded.files.length ? `Loaded ${loaded.files.length} env file${loaded.files.length === 1 ? '' : 's'}.` : 'No env files found.'
      };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
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
    selectedPath = file.path;
    const firstEntry = file.entries[0];
    selectedKey = firstEntry?.key ?? '';
    editValue = firstEntry?.value ?? '';
  }

  function chooseEntry(entry: EnvEntry): void {
    selectedKey = entry.key;
    editValue = entry.value;
  }

  function moveSelection(delta: number): void {
    if (!selectedFile || visibleEntries.length === 0) {
      return;
    }

    const foundIndex = visibleEntries.findIndex((entry) => entry.key === selectedKey);
    const currentIndex = foundIndex === -1 ? (delta > 0 ? -1 : 0) : foundIndex;
    const nextIndex = Math.min(Math.max(currentIndex + delta, 0), visibleEntries.length - 1);
    chooseEntry(visibleEntries[nextIndex]);
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLButtonElement>(`.key-row[data-row-index="${nextIndex}"]`)?.focus();
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
      const refreshed = await saveEnvValue(snapshot?.root ?? (projectPath.trim() || '.'), selectedFile.path, selectedEntry.key, editValue);
      snapshot = snapshot
        ? {
            ...snapshot,
            files: snapshot.files.map((file) => (file.path === refreshed.path ? refreshed : file))
          }
        : null;
      selectedPath = refreshed.path;
      selectedKey = selectedEntry.key;
      const refreshedEntry = refreshed.entries.find((entry) => entry.key === selectedEntry.key);
      editValue = refreshedEntry?.value ?? editValue;
      notice = { kind: 'success', message: `${selectedEntry.key} saved without rewriting the full file view.` };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  async function applyRepairAction(action: EnvRepairAction): Promise<void> {
    if (!canApplyRepairAction(action) || !action.filePath || !action.key) {
      return;
    }

    notice = { kind: 'loading', message: `Adding ${action.key}...` };
    try {
      const root = snapshot?.root ?? (projectPath.trim() || '.');
      await addEnvKey(root, action.filePath, action.key);
      const loaded = await loadProject(root);
      snapshot = loaded;
      selectedPath = action.filePath;
      selectedKey = action.key;
      const refreshedFile = loaded.files.find((file) => file.path === action.filePath);
      const refreshedEntry = refreshedFile?.entries.find((entry) => entry.key === action.key);
      editValue = refreshedEntry?.value ?? '';
      notice = { kind: 'success', message: `${action.key} was added as a blank value.` };
    } catch (error) {
      notice = { kind: 'error', message: errorMessage(error) };
    }
  }

  function canApplyRepairAction(action: EnvRepairAction): boolean {
    return isTauriRuntime() && action.actionKind === 'add-missing-key' && Boolean(action.filePath && action.key);
  }

  function displayValue(entry: EnvEntry): string {
    if (!isEntryValueHidden(entry, showSecrets)) {
      return entry.value || '(empty)';
    }

    return entry.displayValue;
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
        entry.shape.kind.toLowerCase().includes(normalized)
      );
    });
  }

  function profileOrder(profile: FrameworkEnvProfile): string {
    return profile.orderedFiles.map((file) => file.name).join(' -> ') || 'No matching env files';
  }

  function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  void openProject();
</script>

<svelte:head>
  <title>vne</title>
</svelte:head>

<svelte:window onkeydown={handleGlobalKeydown} />

<main class="app-shell">
  <header class="topbar">
    <div class="brand">
      <ShieldCheck size={22} aria-hidden="true" />
      <div>
        <h1>vne</h1>
        <p>Local env files, structured and redacted.</p>
      </div>
    </div>

    <form class="project-form" onsubmit={handleSubmit}>
      <label for="project-path">Project path</label>
      <input id="project-path" bind:value={projectPath} autocomplete="off" spellcheck="false" />
      <button class="primary" type="submit" disabled={notice.kind === 'loading'}>
        <FolderOpen size={16} aria-hidden="true" />
        <span>Open</span>
      </button>
      <button type="button" class="ghost" disabled={notice.kind === 'loading'} onclick={() => void browseProject()}>
        <FolderSearch size={16} aria-hidden="true" />
        <span>Browse</span>
      </button>
      <button type="button" class="icon-button" title="Reload project" aria-label="Reload project" onclick={() => void openProject()}>
        <RefreshCw size={16} aria-hidden="true" />
      </button>
    </form>
  </header>

  <section class="status-strip" class:error={notice.kind === 'error'} class:success={notice.kind === 'success'}>
    {#if notice.kind === 'error'}
      <AlertTriangle size={16} aria-hidden="true" />
    {:else}
      <CheckCircle2 size={16} aria-hidden="true" />
    {/if}
    <span>{notice.message}</span>
    <strong>{issueCount} issue{issueCount === 1 ? '' : 's'}</strong>
  </section>

  <div class="workspace">
    <aside class="file-rail" aria-label="Environment files">
      <div class="rail-heading">
        <span>Files</span>
        <strong>{snapshot?.files.length ?? 0}</strong>
      </div>

      {#if snapshot && snapshot.files.length > 0}
        <div class="file-list">
          {#each snapshot.files as file}
            <button class:active={file.path === selectedPath} type="button" onclick={() => chooseFile(file)}>
              <span>{file.name}</span>
              <small>{file.entries.length} keys</small>
              <em>{file.discoveryReasons[0] ?? 'opened directly'}</em>
            </button>
          {/each}
        </div>
      {:else}
        <p class="empty-copy">No env-like files were found in this directory.</p>
      {/if}

      {#if repairActions.length > 0}
        <div class="comparison-block">
          <h2>Repair queue</h2>
          <ul class="action-list">
            {#each repairActions.slice(0, 5) as action}
              <li class:warning={action.severity === 'warning'}>
                <strong>{action.title}</strong>
                <span>{action.detail}</span>
                {#if action.actionKind === 'add-missing-key'}
                  <button type="button" class="mini-action" disabled={!canApplyRepairAction(action)} onclick={() => void applyRepairAction(action)}>
                    Add blank
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if snapshot?.comparison}
        <div class="comparison-block">
          <h2>Actual vs example</h2>
          <dl>
            <div>
              <dt>Missing</dt>
              <dd>{snapshot.comparison.missingKeys.length}</dd>
            </div>
            <div>
              <dt>Extra</dt>
              <dd>{snapshot.comparison.extraKeys.length}</dd>
            </div>
            <div>
              <dt>Shared</dt>
              <dd>{snapshot.comparison.sharedKeys.length}</dd>
            </div>
          </dl>
        </div>
      {/if}

      {#if snapshot && snapshot.layerReport.orderedFiles.length > 0}
        <div class="comparison-block">
          <h2>Layer diagnostics</h2>
          <dl>
            <div>
              <dt>Layers</dt>
              <dd>{snapshot.layerReport.orderedFiles.length}</dd>
            </div>
            <div>
              <dt>Overrides</dt>
              <dd>{snapshot.layerReport.overrides.length}</dd>
            </div>
            <div>
              <dt>Conflicts</dt>
              <dd>{layerConflicts.length}</dd>
            </div>
          </dl>
          {#if snapshot.layerReport.overrides.length > 0}
            <ul class="override-list">
              {#each snapshot.layerReport.overrides.slice(0, 4) as override}
                <li>
                  <strong>{override.key}</strong>
                  <span>{override.summary}{override.redacted ? ' Value hidden.' : ''}</span>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}

      {#if snapshot && snapshot.frameworkProfiles.length > 0}
        <div class="comparison-block">
          <h2>Framework profiles</h2>
          <dl>
            <div>
              <dt>Profiles</dt>
              <dd>{snapshot.frameworkProfiles.length}</dd>
            </div>
            <div>
              <dt>Missing files</dt>
              <dd>{frameworkMissingCount}</dd>
            </div>
          </dl>
          <ul class="profile-list">
            {#each snapshot.frameworkProfiles.slice(0, 4) as profile}
              <li>
                <strong>{profile.framework} / {profile.mode}</strong>
                <span>{profileOrder(profile)}</span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </aside>

    <section class="table-zone" aria-label="Environment keys">
      <div class="table-toolbar">
        <div class="search-field">
          <Search size={16} aria-hidden="true" />
          <input bind:this={filterInput} bind:value={filter} placeholder="Filter keys or shapes" />
        </div>
        <button type="button" class="ghost" onclick={() => (showSecrets = !showSecrets)}>
          {#if showSecrets}
            <EyeOff size={16} aria-hidden="true" />
            <span>Hide secrets</span>
          {:else}
            <Eye size={16} aria-hidden="true" />
            <span>Reveal</span>
          {/if}
        </button>
      </div>

      <div class="key-table" role="table" aria-label="Parsed env keys">
        <div class="key-row header" role="row">
          <span role="columnheader">Key</span>
          <span role="columnheader">Shape</span>
          <span role="columnheader">Value</span>
          <span role="columnheader">Status</span>
        </div>

        {#if selectedFile && visibleEntries.length > 0}
          {#each visibleEntries as entry, rowIndex}
            {@const status = keyStatus(selectedFile, entry, snapshot?.comparison ?? null)}
            <button
              class="key-row"
              class:selected={entry.key === selectedKey}
              class:warn={status !== 'ok'}
              data-row-index={rowIndex}
              type="button"
              role="row"
              onclick={() => chooseEntry(entry)}
              onkeydown={handleKeyTableKeydown}
            >
              <span class="key-name" role="cell">{entry.key}</span>
              <span class="shape-pill" role="cell">{entry.shape.label}</span>
              <span class:redacted={entry.shape.redactedByDefault && !showSecrets} class="value-cell" role="cell">{displayValue(entry)}</span>
              <span class={`status ${status}`} role="cell">{statusLabel(status)}</span>
            </button>
          {/each}
        {:else}
          <p class="empty-copy">No keys match this view.</p>
        {/if}
      </div>
    </section>

    <aside class="inspector" aria-label="Selected key details">
      {#if selectedFile && selectedEntry}
        {@const selectedStatus = keyStatus(selectedFile, selectedEntry, snapshot?.comparison ?? null)}
        <div class="inspector-heading">
          <span>{selectedFile.name}</span>
          <strong class={`status ${selectedStatus}`}>{statusLabel(selectedStatus)}</strong>
        </div>

        <h2>{selectedEntry.key}</h2>
        <p>{selectedEntry.shape.label} / {selectedEntry.shape.confidence} confidence / line {selectedEntry.lineNumber}</p>
        <p>File found by: {selectedFile.discoveryReasons.join(', ')}</p>

        <label class="editor-label" for="value-editor">Value</label>
        {#if selectedValueHidden}
          <textarea id="value-editor" value={selectedEntry.displayValue} spellcheck="false" rows="5" readonly></textarea>
        {:else}
          <textarea id="value-editor" bind:value={editValue} spellcheck="false" rows="5"></textarea>
        {/if}
        <button class="primary save-button" type="button" disabled={!canSave} onclick={() => void saveValue()}>
          <Save size={16} aria-hidden="true" />
          <span>Save value</span>
        </button>

        <div class="reason-list">
          <h3>Why this shape?</h3>
          {#if selectedEntry.shape.reasons.length}
            <ul>
              {#each selectedEntry.shape.reasons as reason}
                <li>{reason}</li>
              {/each}
            </ul>
          {:else}
            <p>No strong shape signal yet.</p>
          {/if}
        </div>

        <button type="button" class="ghost full-width" disabled={selectedFileHasHiddenSecrets} onclick={() => (showRaw = !showRaw)}>
          {showRaw ? 'Hide raw preview' : 'Show raw preview'}
        </button>
        {#if showRaw}
          <pre>{selectedFile.content}</pre>
        {/if}
      {:else}
        <p class="empty-copy">Select a key to inspect its shape, risk, and editable value.</p>
      {/if}
    </aside>
  </div>
</main>
