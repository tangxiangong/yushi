import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  CompletedTask,
  DownloadTask,
  UpdateInfo,
} from "./types";

/**
 * Tauri Commands
 * Type-safe wrappers for all Tauri backend commands
 */

/**
 * Add a new download task
 * @param url - The URL to download from
 * @param dest - The destination path to save the file
 * @returns The task ID
 */
export async function addTask(url: string, dest: string): Promise<string> {
  return invoke<string>("add_task", { url, dest });
}

/**
 * Get all download tasks
 * @returns Array of all download tasks
 */
export async function getTasks(): Promise<DownloadTask[]> {
  return invoke<DownloadTask[]>("get_tasks");
}

/**
 * Pause a download task
 * @param id - The task ID to pause
 */
export async function pauseTask(id: string): Promise<void> {
  return invoke<void>("pause_task", { id });
}

/**
 * Resume a paused download task
 * @param id - The task ID to resume
 */
export async function resumeTask(id: string): Promise<void> {
  return invoke<void>("resume_task", { id });
}

/**
 * Cancel a download task
 * @param id - The task ID to cancel
 */
export async function cancelTask(id: string): Promise<void> {
  return invoke<void>("cancel_task", { id });
}

/**
 * Remove a download task from the queue
 * @param id - The task ID to remove
 */
export async function removeTask(id: string): Promise<void> {
  return invoke<void>("remove_task", { id });
}

/**
 * Get application configuration
 * @returns Current application configuration
 */
export async function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

/**
 * Update application configuration
 * @param config - New configuration to apply
 */
export async function updateConfig(config: AppConfig): Promise<void> {
  return invoke<void>("update_config", { newConfig: config });
}

/**
 * Get download history
 * @returns Array of completed tasks
 */
export async function getHistory(): Promise<CompletedTask[]> {
  return invoke<CompletedTask[]>("get_history");
}

/**
 * Add a completed task to history
 * @param task - Completed task to add
 */
export async function addToHistory(task: CompletedTask): Promise<void> {
  return invoke<void>("add_to_history", { task });
}

/**
 * Remove a task from history
 * @param id - History item ID to remove
 */
export async function removeFromHistory(id: string): Promise<void> {
  return invoke<void>("remove_from_history", { id });
}

/**
 * Clear all download history
 */
export async function clearHistory(): Promise<void> {
  return invoke<void>("clear_history");
}

/**
 * Search download history
 * @param query - Search query string
 * @returns Matching history items
 */
export async function searchHistory(query: string): Promise<CompletedTask[]> {
  return invoke<CompletedTask[]>("search_history", { query });
}

/**
 * Check for application updates
 * @returns Update information
 */
export async function checkForUpdates(): Promise<UpdateInfo> {
  return invoke<UpdateInfo>("check_for_updates");
}

/**
 * Download and install update
 */
export async function downloadAndInstallUpdate(): Promise<void> {
  return invoke<void>("download_and_install_update");
}
