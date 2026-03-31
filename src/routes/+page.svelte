<script lang="ts">
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";

  let updateStatus = $state("");
  let updateAvailable = $state(false);
  let isUpdating = $state(false);

  async function checkForUpdate() {
    try {
      updateStatus = "Checking for updates...";
      const update = await check();
      if (update) {
        updateAvailable = true;
        updateStatus = `Update available: v${update.version}`;
      } else {
        updateStatus = "You're on the latest version!";
      }
    } catch (e) {
      updateStatus = `Update check failed: ${e}`;
    }
  }

  async function installUpdate() {
    try {
      isUpdating = true;
      updateStatus = "Downloading update...";
      const update = await check();
      if (update) {
        await update.downloadAndInstall((event) => {
          switch (event.event) {
            case 'Started':
              updateStatus = `Downloading... (${event.data.contentLength} bytes)`;
              break;
            case 'Progress':
              updateStatus = `Downloading...`;
              break;
            case 'Finished':
              updateStatus = "Download complete. Restarting...";
              break;
          }
        });
        await relaunch();
      }
    } catch (e) {
      updateStatus = `Update failed: ${e}`;
      isUpdating = false;
    }
  }
</script>

<main class="container">
  <div class="update-section">
    {#if !updateAvailable}
      <button onclick={checkForUpdate}>Check for Updates</button>
    {:else}
      <button onclick={installUpdate} disabled={isUpdating}>
        {isUpdating ? 'Updating...' : 'Install Update'}
      </button>
    {/if}
    {#if updateStatus}
      <p class="update-status">{updateStatus}</p>
    {/if}
  </div>
</main>

<style>
:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #0f0f0f;
  background-color: #f6f6f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

.container {
  margin: 0;
  padding-top: 10vh;
  display: flex;
  flex-direction: column;
  justify-content: center;
  text-align: center;
}

button {
  border-radius: 8px;
  border: 1px solid transparent;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  font-family: inherit;
  color: #0f0f0f;
  background-color: #ffffff;
  transition: border-color 0.25s;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
  cursor: pointer;
  outline: none;
}

button:hover {
  border-color: #396cd8;
}
button:active {
  border-color: #396cd8;
  background-color: #e8e8e8;
}

.update-section {
  margin-top: 2rem;
  padding-top: 1.5rem;
}

.update-status {
  font-size: 0.9em;
  opacity: 0.8;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }

  button {
    color: #ffffff;
    background-color: #0f0f0f98;
  }
  button:active {
    background-color: #0f0f0f69;
  }
}

</style>
