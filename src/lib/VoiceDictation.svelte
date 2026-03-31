<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import * as whisper from "./whisper";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let status = $state<whisper.WhisperStatus | null>(null);
  let isRecording = $state(false);
  let isProcessing = $state(false);
  let recordingTime = $state(0);
  let transcribedText = $state("");
  let error = $state("");
  let copied = $state(false);

  let mediaRecorder: MediaRecorder | null = null;
  let audioChunks: Blob[] = [];
  let recordingInterval: ReturnType<typeof setInterval>;
  let unlisteners: UnlistenFn[] = [];
  let healthInterval: ReturnType<typeof setInterval>;

  onMount(async () => {
    await refreshStatus();

    // Listen for server status changes so the badge stays reactive
    unlisteners.push(
      await whisper.onServerLog(() => {
        // When the server emits logs it might have just started — recheck
        debouncedRefresh();
      })
    );
    unlisteners.push(
      await whisper.onServerStopped(() => {
        if (status) {
          status = { ...status, server_running: false, server_healthy: false };
        }
      })
    );

    // Periodic health check every 4s
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
    }, 4000);
  });

  onDestroy(() => {
    for (const unlisten of unlisteners) unlisten();
    clearInterval(healthInterval);
    clearInterval(recordingInterval);
    // Clean up any active recording
    if (mediaRecorder && mediaRecorder.state !== "inactive") {
      mediaRecorder.stop();
      mediaRecorder.stream.getTracks().forEach((t) => t.stop());
    }
  });

  let refreshTimer: ReturnType<typeof setTimeout>;
  function debouncedRefresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(refreshStatus, 1000);
  }

  async function refreshStatus() {
    try {
      status = await whisper.getStatus();
    } catch (e) {
      // silently ignore — WhisperManager owns the main status
    }
  }

  // ─── WAV encoding helpers ─────────────────────────────────────────────────
  // Browsers record as webm/ogg which miniaudio may not decode from memory.
  // We decode with AudioContext and re-encode as 16-bit PCM WAV so the server
  // can always handle it — even without --convert / ffmpeg.

  function encodeWav(samples: Float32Array, sampleRate: number): Blob {
    const numChannels = 1;
    const bitsPerSample = 16;
    const byteRate = sampleRate * numChannels * (bitsPerSample / 8);
    const blockAlign = numChannels * (bitsPerSample / 8);
    const dataSize = samples.length * (bitsPerSample / 8);
    const buffer = new ArrayBuffer(44 + dataSize);
    const view = new DataView(buffer);

    // RIFF header
    writeString(view, 0, "RIFF");
    view.setUint32(4, 36 + dataSize, true);
    writeString(view, 8, "WAVE");

    // fmt chunk
    writeString(view, 12, "fmt ");
    view.setUint32(16, 16, true); // chunk size
    view.setUint16(20, 1, true); // PCM
    view.setUint16(22, numChannels, true);
    view.setUint32(24, sampleRate, true);
    view.setUint32(28, byteRate, true);
    view.setUint16(32, blockAlign, true);
    view.setUint16(34, bitsPerSample, true);

    // data chunk
    writeString(view, 36, "data");
    view.setUint32(40, dataSize, true);

    // Convert float32 [-1,1] → int16
    let offset = 44;
    for (let i = 0; i < samples.length; i++) {
      const s = Math.max(-1, Math.min(1, samples[i]));
      view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
      offset += 2;
    }

    return new Blob([buffer], { type: "audio/wav" });
  }

  function writeString(view: DataView, offset: number, str: string) {
    for (let i = 0; i < str.length; i++) {
      view.setUint8(offset + i, str.charCodeAt(i));
    }
  }

  async function blobToWav(blob: Blob): Promise<Blob> {
    const arrayBuffer = await blob.arrayBuffer();
    const audioCtx = new AudioContext({ sampleRate: 16000 });
    try {
      const decoded = await audioCtx.decodeAudioData(arrayBuffer);
      // Mix down to mono if needed
      const mono = decoded.getChannelData(0);
      return encodeWav(mono, decoded.sampleRate);
    } finally {
      await audioCtx.close();
    }
  }

  // ─── Recording ────────────────────────────────────────────────────────────

  async function startRecording() {
    error = "";
    transcribedText = "";
    recordingTime = 0;
    copied = false;

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      mediaRecorder = new MediaRecorder(stream);
      audioChunks = [];

      mediaRecorder.ondataavailable = (event) => {
        if (event.data.size > 0) {
          audioChunks.push(event.data);
        }
      };

      mediaRecorder.start();
      isRecording = true;

      recordingInterval = setInterval(() => {
        recordingTime += 0.1;
      }, 100);
    } catch (e) {
      error = `Microphone access denied or unavailable: ${e}`;
    }
  }

  async function stopRecording() {
    if (!mediaRecorder || !status) return;

    isRecording = false;
    clearInterval(recordingInterval);

    // Wrap in a promise so we can await the onstop callback
    const audioBlob = await new Promise<Blob>((resolve) => {
      mediaRecorder!.onstop = () => {
        const mimeType = mediaRecorder!.mimeType || "audio/webm";
        resolve(new Blob(audioChunks, { type: mimeType }));
      };
      mediaRecorder!.stop();
    });

    // Release the mic immediately
    mediaRecorder!.stream.getTracks().forEach((t) => t.stop());
    mediaRecorder = null;
    audioChunks = [];

    // Transcribe
    isProcessing = true;
    try {
      // Convert to WAV so the server can always decode it
      let wavBlob: Blob;
      try {
        wavBlob = await blobToWav(audioBlob);
      } catch {
        // Fallback: send raw blob and hope --convert / ffmpeg handles it
        wavBlob = audioBlob;
      }

      const formData = new FormData();
      formData.append("file", wavBlob, "recording.wav");
      formData.append("response_format", "json");
      formData.append("temperature", "0.0");

      const response = await fetch(
        `http://127.0.0.1:${status.port}/inference`,
        {
          method: "POST",
          body: formData,
        }
      );

      if (!response.ok) {
        const body = await response.text();
        throw new Error(`Server returned ${response.status}: ${body}`);
      }

      const result = await response.json();

      if (result.error) {
        throw new Error(result.error);
      }

      transcribedText = (result.text || "").trim();

      if (!transcribedText) {
        error = "No speech was detected. Try speaking louder or longer.";
      }
    } catch (e) {
      error = `Transcription failed: ${e}`;
    } finally {
      isProcessing = false;
    }
  }

  function formatTime(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  }

  async function copyToClipboard() {
    try {
      await navigator.clipboard.writeText(transcribedText);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      // Fallback
      const el = document.createElement("textarea");
      el.value = transcribedText;
      document.body.appendChild(el);
      el.select();
      document.execCommand("copy");
      document.body.removeChild(el);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    }
  }
</script>

<div class="voice-dictation">
  <!-- Header -->
  <div class="header">
    <h3>Voice Dictation</h3>
    {#if status?.server_healthy}
      <span class="status-badge healthy">Ready</span>
    {:else if status?.server_running}
      <span class="status-badge running">Starting...</span>
    {:else}
      <span class="status-badge unavailable">Server offline</span>
    {/if}
  </div>

  <!-- Error banner -->
  {#if error}
    <div class="error-banner">
      <span>{error}</span>
      <button class="dismiss" onclick={() => (error = "")}>&#215;</button>
    </div>
  {/if}

  <!-- Hint when server is not available -->
  {#if status && !status.server_healthy && !isRecording && !isProcessing}
    <p class="hint">Start the whisper.cpp server above to enable dictation.</p>
  {/if}

  <!-- Recording controls -->
  <div class="controls">
    {#if !isRecording}
      <button
        onclick={startRecording}
        disabled={!status?.server_healthy || isProcessing}
        class="record-btn"
      >
        <svg class="mic-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/>
          <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
          <line x1="12" y1="19" x2="12" y2="22"/>
        </svg>
        Dictate
      </button>
    {:else}
      <div class="recording-row">
        <div class="recording-info">
          <span class="recording-indicator"></span>
          <span class="recording-time">{formatTime(recordingTime)}</span>
        </div>
        <button onclick={stopRecording} class="stop-btn">
          <svg class="stop-icon" viewBox="0 0 24 24" fill="currentColor">
            <rect x="6" y="6" width="12" height="12" rx="1"/>
          </svg>
          Stop
        </button>
      </div>
    {/if}
  </div>

  <!-- Processing indicator -->
  {#if isProcessing}
    <div class="processing">
      <div class="spinner"></div>
      <span>Transcribing...</span>
    </div>
  {/if}

  <!-- Transcribed text display -->
  {#if transcribedText}
    <div class="result-section">
      <div class="result-header">
        <h4>Transcription</h4>
        <button class="copy-btn" onclick={copyToClipboard} title="Copy to clipboard">
          {copied ? "Copied!" : "Copy"}
        </button>
      </div>
      <div class="result-text">
        {transcribedText}
      </div>
    </div>
  {/if}
</div>

<style>
  .voice-dictation {
    border: 1px solid rgba(127, 127, 127, 0.2);
    border-radius: 10px;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    background: rgba(127, 127, 127, 0.04);
  }

  /* Header */
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
  .header h3 {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    opacity: 0.85;
  }

  .status-badge {
    display: inline-block;
    padding: 0.2em 0.6em;
    border-radius: 999px;
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }
  .status-badge.healthy {
    background: #16a34a;
    color: #fff;
  }
  .status-badge.running {
    background: #eab308;
    color: #000;
  }
  .status-badge.unavailable {
    background: #6b7280;
    color: #fff;
  }

  /* Hint */
  .hint {
    font-size: 0.82rem;
    opacity: 0.55;
    margin: 0;
    font-style: italic;
  }

  /* Error banner */
  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: #dc2626;
    color: #fff;
    padding: 0.5rem 0.8rem;
    border-radius: 8px;
    font-size: 0.82rem;
    gap: 0.8rem;
  }
  .error-banner .dismiss {
    background: transparent;
    border: none;
    color: #fff;
    font-size: 1.2rem;
    cursor: pointer;
    padding: 0;
    box-shadow: none;
    line-height: 1;
    flex-shrink: 0;
  }

  /* Controls */
  .controls {
    display: flex;
    gap: 0.8rem;
    align-items: center;
  }

  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.5em 1.2em;
    font-size: 0.9rem;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.15s;
    outline: none;
    color: inherit;
    background: rgba(127, 127, 127, 0.12);
  }
  button:hover:not(:disabled) {
    background: rgba(127, 127, 127, 0.2);
  }
  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .record-btn {
    background: #2563eb;
    color: #fff;
    display: flex;
    align-items: center;
    gap: 0.5em;
    padding: 0.6em 1.4em;
    font-size: 0.95rem;
  }
  .record-btn:hover:not(:disabled) {
    background: #1d4ed8;
  }
  .mic-icon {
    width: 1.1em;
    height: 1.1em;
    flex-shrink: 0;
  }

  .recording-row {
    display: flex;
    align-items: center;
    gap: 1rem;
    width: 100%;
  }

  .stop-btn {
    background: #dc2626;
    color: #fff;
    display: flex;
    align-items: center;
    gap: 0.4em;
    padding: 0.6em 1.4em;
    font-size: 0.95rem;
  }
  .stop-btn:hover {
    background: #b91c1c;
  }
  .stop-icon {
    width: 1em;
    height: 1em;
    flex-shrink: 0;
  }

  /* Recording info */
  .recording-info {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    font-weight: 500;
  }
  .recording-indicator {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #dc2626;
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
      transform: scale(1);
    }
    50% {
      opacity: 0.4;
      transform: scale(0.85);
    }
  }

  .recording-time {
    font-family: "SF Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 1.1em;
    color: #dc2626;
    min-width: 3ch;
  }

  /* Processing */
  .processing {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.8rem;
    padding: 0.8rem;
    color: #2563eb;
    font-weight: 500;
    font-size: 0.9rem;
  }
  .spinner {
    width: 18px;
    height: 18px;
    border: 2px solid rgba(37, 99, 235, 0.2);
    border-top: 2px solid #2563eb;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Result section */
  .result-section {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .result-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .result-header h4 {
    margin: 0;
    font-size: 0.88rem;
    font-weight: 600;
    opacity: 0.85;
  }
  .copy-btn {
    padding: 0.25em 0.7em;
    font-size: 0.78rem;
    background: rgba(37, 99, 235, 0.12);
    color: #2563eb;
    border: 1px solid rgba(37, 99, 235, 0.25);
  }
  .copy-btn:hover {
    background: rgba(37, 99, 235, 0.22);
  }

  .result-text {
    background: rgba(127, 127, 127, 0.08);
    border: 1px solid rgba(127, 127, 127, 0.2);
    border-radius: 8px;
    padding: 0.8rem 1rem;
    line-height: 1.6;
    word-break: break-word;
    font-size: 0.92rem;
    white-space: pre-wrap;
    min-height: 2.5em;
    user-select: text;
  }

  @media (prefers-color-scheme: dark) {
    .voice-dictation {
      background: rgba(127, 127, 127, 0.06);
    }
    .result-text {
      background: rgba(0, 0, 0, 0.2);
    }
    .recording-time {
      color: #f87171;
    }
    .recording-indicator {
      background: #f87171;
    }
    .processing {
      color: #60a5fa;
    }
    .spinner {
      border-color: rgba(96, 165, 250, 0.2);
      border-top-color: #60a5fa;
    }
    .copy-btn {
      color: #60a5fa;
      background: rgba(96, 165, 250, 0.12);
      border-color: rgba(96, 165, 250, 0.25);
    }
    .copy-btn:hover {
      background: rgba(96, 165, 250, 0.22);
    }
  }
</style>
