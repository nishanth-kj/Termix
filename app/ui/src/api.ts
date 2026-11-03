// Typed access to the Rust commands in app/src-tauri/src/lib.rs.

import { Channel, invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";

export interface Machine {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  last_connected_at: string | null;
  has_saved_password: boolean;
  pinned_fingerprint: string | null;
}

export interface Credentials {
  password?: string | null;
  keyPath?: string | null;
  keyPassphrase?: string | null;
  remember?: boolean;
}

/** Matches `AppError` in session.rs. */
export type AppError =
  | { kind: "host_key_unknown"; message: string; algorithm: string; fingerprint: string }
  | { kind: "host_key_changed"; message: string }
  | { kind: "needs_confirmation"; message: string }
  | { kind: "needs_credentials"; message: string; failed: boolean }
  | { kind: "other"; message: string };

export interface CommandOutput {
  stdout: string;
  stderr: string;
  exit_code: number | null;
}

export interface RemoteEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  modified_unix: number;
}

export interface Listing {
  path: string;
  entries: RemoteEntry[];
}

export interface Sample {
  source: string;
  cpu_percent: number | null;
  memory_total_bytes: number | null;
  memory_used_bytes: number | null;
  uptime_seconds: number | null;
  disks: { filesystem: string; mount: string; total_kib: number; used_kib: number }[];
  processes: { pid: number; name: string; memory_bytes: number }[];
}

export interface SyncPlan {
  copies: { relative_path: string; reason: string }[];
  conflicts: string[];
  would_delete: string[];
}

export interface SupportRow {
  name: string;
  implemented: boolean;
}

export type ShellEvent = { kind: "output"; data: number[] } | { kind: "closed" };

export function shellChannel(onEvent: (event: ShellEvent) => void): Channel<ShellEvent> {
  const channel = new Channel<ShellEvent>();
  channel.onmessage = onEvent;
  return channel;
}

export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    return Promise.reject(new Error("Open this screen inside the Termix app. It is not a website."));
  }
  return tauriInvoke<T>(command, args);
}

export function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) return String(error.message);
  return String(error);
}

export function appErrorOf(error: unknown): AppError | null {
  return typeof error === "object" && error !== null && "kind" in error ? (error as AppError) : null;
}
