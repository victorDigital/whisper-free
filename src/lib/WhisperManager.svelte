<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import * as whisper from "./whisper";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let status = $state<whisper.WhisperStatus | null>(null);
  let deps = $state<whisper.DepsStatus | null>(null);
  let logs = $state<string[]>([]);
  let isInstalling = $state(false);
  let isUpdating = $state(false);
  let selectedModel = $state("base.en");
  let selectedPort = $state(8080);
  let error = $state("");
  let showLogs = $state(false);

  let unlisteners: UnlistenFn[] = [];
  let healthInterval: ReturnType<typeof setInterval>;
  let logContainer: HTMLElement = $state(null!);

  onMount(async () => {
    await refreshStatus();
    await refreshDeps();

    unlisteners.push(
      await whisper.onProgress((payload) => {
        addLog(`[${payload.stage}] ${payload.message}`);
        if (payload.done) {
          isInstalling = false;
          isUpdating = false;
          refreshStatus();
        }
        if (payload.error) {
          error = payload.message;
        }
      })
    );

    unlisteners.push(
      await whisper.onServerLog((payload) => {
        addLog(`[${payload.source}] ${payload.message}`);
      })
    );

    unlisteners.push(
      await whisper.onServerStopped((payload) => {
        addLog(
          `[server] Stopped (code: ${payload.code ?? "?"}, signal: ${payload.signal ?? "none"})`
        );
        refreshStatus();
      })
    );

    // Periodic health check every 5s when server is supposed to be running
    healthInterval = setInterval(async () => {
      if (status?.server_running) {
        try {
          const healthy = await whisper.healthCheck();
          if (status) {
            status = { ...status, server_healthy: healthy };
          }
        } catch {
          // ignore
        }
      }
    }, 5000);
  });

  onDestroy(() => {
    for (const unlisten of unlisteners) unlisten();
    clearInterval(healthInterval);
  });

  async function refreshStatus() {
    try {
      status = await whisper.getStatus();
      selectedPort = status.port;
      selectedModel = status.model;
    } catch (e) {
      error = `Failed to get status: ${e}`;
    }
  }

  async function refreshDeps() {
    try {
      deps = await whisper.checkDeps();
    } catch (e) {
      error = `Failed to check dependencies: ${e}`;
    }
  }

  function addLog(msg: string) {
    const ts = new Date().toLocaleTimeString();
    logs = [...logs.slice(-500), `[${ts}] ${msg}`];
    // Auto-scroll log container
    requestAnimationFrame(() => {
      if (logContainer) {
        logContainer.scrollTop = logContainer.scrollHeight;
      }
    });
  }

  async function handleInstall() {
    error = "";
    isInstalling = true;
    showLogs = true;
    addLog("Starting installation...");
    try {
      await whisper.install(selectedModel);
    } catch (e) {
      error = `Installation failed: ${e}`;
      isInstalling = false;
    }
  }

  async function handleStart() {
    error = "";
    showLogs = true;
    try {
      await whisper.startServer(selectedPort);
      addLog(`Server starting on port ${selectedPort}...`);
      setTimeout(refreshStatus, 2000);
    } catch (e) {
      error = `Failed to start: ${e}`;
    }
  }

  async function handleStop() {
    error = "";
    try {
      await whisper.stopServer();
      addLog("Server stopped");
      await refreshStatus();
    } catch (e) {
      error = `Failed to stop: ${e}`;
    }
  }

  async function handleUpdate() {
    error = "";
    isUpdating = true;
    showLogs = true;
    addLog("Starting update...");
    try {
      await whisper.update();
    } catch (e) {
      error = `Update failed: ${e}`;
      isUpdating = false;
    }
  }

  async function handleRemove() {
    error = "";
    try {
      await whisper.remove();
      addLog("whisper.cpp removed");
      await refreshStatus();
    } catch (e) {
      error = `Failed to remove: ${e}`;
    }
  }
</script>

<div class="whisper-manager">
  <!-- ─── Status Badge ──────────────────────────────────────────── -->
  <div class="status-bar">
    <h2>whisper.cpp</h2>
    {#if status}
      <div class="badges">
        {#if !status.installed}
          <span class="badge not-installed">Not Installed</span>
        {:else if status.server_healthy}
          <span class="badge healthy">Healthy</span>
        {:else if status.server_running}
          <span class="badge running">Starting...</span>
        {:else}
          <span class="badge installed">Installed</span>
        {/if}
      </div>
    {:else}
      <span class="badge loading">Loading...</span>
    {/if}
  </div>

  <!-- ─── Error Banner ──────────────────────────────────────────── -->
  {#if error}
    <div class="error-banner">
      <span>{error}</span>
      <button class="dismiss" onclick={() => (error = "")}>x</button>
    </div>
  {/if}

  <!-- ─── Dependencies ──────────────────────────────────────────── -->
  {#if deps && !deps.all_ok}
    <div class="section deps-warning">
      <h3>Missing Dependencies</h3>
      <p>Install these before proceeding:</p>
      <ul>
        {#if !deps.git}<li class="missing">git</li>{/if}
        {#if !deps.cmake}<li class="missing">cmake</li>{/if}
        {#if !deps.cc}<li class="missing">C compiler (cc/clang/gcc)</li>{/if}
        {#if !deps.ffmpeg}<li class="missing">ffmpeg (optional but recommended)</li>{/if}
      </ul>
      <p class="hint">
        On macOS: <code>brew install cmake ffmpeg</code>
      </p>
    </div>
  {/if}

  <!-- ─── Install Section ───────────────────────────────────────── -->
  {#if status && !status.installed}
    <div class="section">
      <h3>Install whisper.cpp</h3>
      <div class="controls">
        <label>
          Model
          <select bind:value={selectedModel} disabled={isInstalling}>
            {#each whisper.AVAILABLE_MODELS as model}
              <option value={model}>{model}</option>
            {/each}
          </select>
        </label>
        <button
          onclick={handleInstall}
          disabled={isInstalling || (deps !== null && !deps.all_ok)}
          class="primary"
        >
          {isInstalling ? "Installing..." : "Install"}
        </button>
      </div>
      {#if isInstalling}
        <div class="progress-bar"><div class="indeterminate"></div></div>
      {/if}
    </div>
  {/if}

  <!-- ─── Server Controls ───────────────────────────────────────── -->
  {#if status?.installed}
    <div class="section">
      <h3>Server</h3>
      <div class="controls">
        <label>
          Port
          <input
            type="number"
            bind:value={selectedPort}
            disabled={status.server_running}
            min="1024"
            max="65535"
          />
        </label>
        {#if !status.server_running}
          <button onclick={handleStart} class="primary">Start Server</button>
        {:else}
          <button onclick={handleStop} class="danger">Stop Server</button>
        {/if}
      </div>
      {#if status.server_running && status.server_healthy}
        <p class="info">
          Listening on <code>http://127.0.0.1:{status.port}</code>
        </p>
      {/if}
    </div>
  {/if}

  <!-- ─── Management ────────────────────────────────────────────── -->
  {#if status?.installed}
    <div class="section">
      <h3>Manage</h3>
      <div class="controls">
        <button onclick={handleUpdate} disabled={isUpdating || status.server_running}>
          {isUpdating ? "Updating..." : "Update"}
        </button>
        <button onclick={handleRemove} class="danger" disabled={isUpdating}>
          Remove
        </button>
        <button onclick={refreshStatus} class="subtle">Refresh</button>
      </div>
      {#if isUpdating}
        <div class="progress-bar"><div class="indeterminate"></div></div>
      {/if}
      {#if status}
        <p class="meta">
          Model: <strong>{status.model}</strong> &middot; Path: <code>{status.install_path}</code>
        </p>
      {/if}
    </div>
  {/if}

  <!-- ─── Log Output ────────────────────────────────────────────── -->
  <div class="section">
    <button class="subtle toggle-logs" onclick={() => (showLogs = !showLogs)}>
      {showLogs ? "Hide" : "Show"} Logs ({logs.length})
    </button>
    {#if showLogs}
      <div class="log-container" bind:this={logContainer}>
        {#each logs as line}
          <div class="log-line">{line}</div>
        {/each}
        {#if logs.length === 0}
          <div class="log-line empty">No log output yet.</div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .whisper-manager {
    width: 100%;
    max-width: 640px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* Status Bar */
  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
  .status-bar h2 {
    margin: 0;
    font-size: 1.3rem;
    font-weight: 600;
  }

  /* Badges */
  .badge {
    display: inline-block;
    padding: 0.2em 0.7em;
    border-radius: 999px;
    font-size: 0.8rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .badge.not-installed {
    background: #555;
    color: #ddd;
  }
  .badge.installed {
    background: #2563eb;
    color: #fff;
  }
  .badge.running {
    background: #eab308;
    color: #000;
  }
  .badge.healthy {
    background: #16a34a;
    color: #fff;
  }
  .badge.loading {
    background: #6b7280;
    color: #fff;
  }

  /* Error Banner */
  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: #dc2626;
    color: #fff;
    padding: 0.6rem 1rem;
    border-radius: 8px;
    font-size: 0.85rem;
  }
  .error-banner .dismiss {
    background: transparent;
    border: none;
    color: #fff;
    font-size: 1rem;
    cursor: pointer;
    padding: 0 0.3rem;
    box-shadow: none;
  }

  /* Sections */
  .section {
    border: 1px solid rgba(127, 127, 127, 0.2);
    border-radius: 10px;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .section h3 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    opacity: 0.85;
  }

  /* Dependencies warning */
  .deps-warning {
    border-color: #eab308;
    background: rgba(234, 179, 8, 0.08);
  }
  .deps-warning ul {
    margin: 0;
    padding-left: 1.2rem;
  }
  .deps-warning .missing {
    color: #dc2626;
    font-weight: 500;
  }
  .hint {
    font-size: 0.82rem;
    opacity: 0.7;
  }
  .hint code {
    background: rgba(127, 127, 127, 0.15);
    padding: 0.15em 0.4em;
    border-radius: 4px;
    font-size: 0.85em;
  }

  /* Controls row */
  .controls {
    display: flex;
    align-items: flex-end;
    gap: 0.6rem;
    flex-wrap: wrap;
  }
  .controls label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.82rem;
    font-weight: 500;
    opacity: 0.8;
  }
  .controls select,
  .controls input[type="number"] {
    padding: 0.45em 0.6em;
    border-radius: 6px;
    border: 1px solid rgba(127, 127, 127, 0.3);
    background: rgba(127, 127, 127, 0.08);
    color: inherit;
    font-size: 0.9rem;
    min-width: 100px;
  }

  /* Buttons */
  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.5em 1em;
    font-size: 0.9rem;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s,
      opacity 0.15s;
    outline: none;
    color: inherit;
    background: rgba(127, 127, 127, 0.12);
  }
  button:hover {
    background: rgba(127, 127, 127, 0.2);
  }
  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  button.primary {
    background: #2563eb;
    color: #fff;
  }
  button.primary:hover:not(:disabled) {
    background: #1d4ed8;
  }
  button.danger {
    background: #dc2626;
    color: #fff;
  }
  button.danger:hover:not(:disabled) {
    background: #b91c1c;
  }
  button.subtle {
    background: transparent;
    opacity: 0.7;
    font-size: 0.82rem;
    padding: 0.3em 0.6em;
  }
  button.subtle:hover {
    opacity: 1;
    background: rgba(127, 127, 127, 0.1);
  }

  /* Progress bar */
  .progress-bar {
    height: 3px;
    border-radius: 2px;
    background: rgba(127, 127, 127, 0.15);
    overflow: hidden;
  }
  .indeterminate {
    height: 100%;
    width: 40%;
    background: #2563eb;
    border-radius: 2px;
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(350%);
    }
  }

  /* Info / meta text */
  .info,
  .meta {
    font-size: 0.82rem;
    opacity: 0.7;
    margin: 0;
  }
  .meta code {
    background: rgba(127, 127, 127, 0.15);
    padding: 0.1em 0.35em;
    border-radius: 4px;
    font-size: 0.85em;
    word-break: break-all;
  }

  /* Log container */
  .toggle-logs {
    align-self: flex-start;
  }
  .log-container {
    max-height: 220px;
    overflow-y: auto;
    background: rgba(0, 0, 0, 0.6);
    color: #d4d4d4;
    border-radius: 6px;
    padding: 0.5rem;
    font-family: "SF Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 0.75rem;
    line-height: 1.5;
  }
  .log-line {
    white-space: pre-wrap;
    word-break: break-all;
  }
  .log-line.empty {
    opacity: 0.4;
    font-style: italic;
  }
</style>
