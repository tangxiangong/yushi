export type TaskStatus =
  | "Pending"
  | "Downloading"
  | "Paused"
  | "Completed"
  | "Failed"
  | "Cancelled";

export type Priority = "Low" | "Normal" | "High";

export interface ChecksumType {
  Md5?: string;
  Sha256?: string;
}

export interface DownloadTask {
  id: string;
  url: string;
  dest: string;
  status: TaskStatus;
  total_size: number;
  downloaded: number;
  created_at: number;
  error?: string;
  priority: Priority;
  speed: number;
  eta?: number;
  headers: Record<string, string>;
  checksum?: ChecksumType;
}

export type QueueEvent =
  | { type: "TaskAdded"; payload: { task_id: string } }
  | { type: "TaskStarted"; payload: { task_id: string } }
  | {
    type: "TaskProgress";
    payload: {
      task_id: string;
      downloaded: number;
      total: number;
      speed: number;
      eta?: number;
    };
  }
  | { type: "TaskCompleted"; payload: { task_id: string } }
  | { type: "TaskFailed"; payload: { task_id: string; error: string } }
  | { type: "TaskPaused"; payload: { task_id: string } }
  | { type: "TaskResumed"; payload: { task_id: string } }
  | { type: "TaskCancelled"; payload: { task_id: string } }
  | { type: "VerifyStarted"; payload: { task_id: string } }
  | { type: "VerifyCompleted"; payload: { task_id: string; success: boolean } };

/**
 * Window state
 */
export interface WindowState {
  /** Window width */
  width: number;
  /** Window height */
  height: number;
  /** Window X position */
  x: number;
  /** Window Y position */
  y: number;
  /** Is window maximized */
  maximized: boolean;
  /** Is sidebar open */
  sidebar_open: boolean;
}

/**
 * Application configuration
 */
export interface AppConfig {
  /** Default download directory path */
  default_download_path: string;
  /** Maximum concurrent download connections per task */
  max_concurrent_downloads: number;
  /** Maximum concurrent tasks in the queue */
  max_concurrent_tasks: number;
  /** Chunk size in bytes */
  chunk_size: number;
  /** Connection timeout in seconds */
  timeout: number;
  /** User agent string */
  user_agent: string;
  /** Theme setting (light, dark, system) */
  theme: string;
  /** Window state */
  window: WindowState;
}

/**
 * Completed download task (history record)
 */
export interface CompletedTask {
  /** Task ID */
  id: string;
  /** Download URL */
  url: string;
  /** Destination path */
  dest: string;
  /** Total file size in bytes */
  total_size: number;
  /** Completion timestamp */
  completed_at: number;
  /** Download duration in seconds */
  duration: number;
  /** Average download speed in bytes/second */
  avg_speed: number;
}

/**
 * Update information
 */
export interface UpdateInfo {
  /** Is update available */
  available: boolean;
  /** Current version */
  current_version: string;
  /** Latest version */
  latest_version?: string;
  /** Release notes */
  body?: string;
  /** Release date */
  date?: string;
}
