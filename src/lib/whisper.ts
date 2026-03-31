import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ─── Types ───────────────────────────────────────────────────────────────────

export interface WhisperStatus {
  installed: boolean;
  server_running: boolean;
  server_healthy: boolean;
  port: number;
  model: string;
  install_path: string;
  binary_exists: boolean;
  model_exists: boolean;
}

export interface DepsStatus {
  git: boolean;
  cmake: boolean;
  ffmpeg: boolean;
  cc: boolean;
  all_ok: boolean;
}

export interface ProgressPayload {
  stage: string;
  message: string;
  done: boolean;
  error: boolean;
}

export interface LogPayload {
  source: "stdout" | "stderr" | "error";
  message: string;
}

// ─── Invoke wrappers ────────────────────────────────────────────────────────

export async function getStatus(): Promise<WhisperStatus> {
  return invoke("whisper_get_status");
}

export async function checkDeps(): Promise<DepsStatus> {
  return invoke("whisper_check_deps");
}

export async function install(model?: string): Promise<void> {
  return invoke("whisper_install", { model });
}

export async function startServer(port?: number): Promise<void> {
  return invoke("whisper_start_server", { port });
}

export async function stopServer(): Promise<void> {
  return invoke("whisper_stop_server");
}

export async function healthCheck(): Promise<boolean> {
  return invoke("whisper_health_check");
}

export async function update(): Promise<void> {
  return invoke("whisper_update");
}

export async function remove(): Promise<void> {
  return invoke("whisper_remove");
}

// ─── Event listeners ─────────────────────────────────────────────────────────

export function onProgress(
  callback: (payload: ProgressPayload) => void
): Promise<UnlistenFn> {
  return listen<ProgressPayload>("whisper://progress", (event) =>
    callback(event.payload)
  );
}

export function onServerLog(
  callback: (payload: LogPayload) => void
): Promise<UnlistenFn> {
  return listen<LogPayload>("whisper://server-log", (event) =>
    callback(event.payload)
  );
}

export function onServerStopped(
  callback: (payload: { code?: number; signal?: number }) => void
): Promise<UnlistenFn> {
  return listen<{ code?: number; signal?: number }>(
    "whisper://server-stopped",
    (event) => callback(event.payload)
  );
}

// ─── Constants ───────────────────────────────────────────────────────────────

export const AVAILABLE_MODELS = [
  "tiny",
  "tiny.en",
  "base",
  "base.en",
  "small",
  "small.en",
  "medium",
  "medium.en",
  "large-v1",
  "large-v2",
  "large-v3",
  "large-v3-turbo",
] as const;
