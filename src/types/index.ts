export interface ParsedDriveLink {
  is_valid: boolean;
  is_folder: boolean;
  is_file: boolean;
  id: string;
  resource_key?: string | null;
  connection_string: string;
  error?: string | null;
}

export interface StorageInfo {
  path: string;
  mount_point: string;
  total_bytes: number;
  free_bytes: number;
  free_gb: number;
  total_gb: number;
  file_system: string;
  is_writable: boolean;
  is_fat32: boolean;
  error?: string | null;
}

export interface TransferringFile {
  name: string;
  bytes: number;
  size: number;
  percentage: number;
  speed: number;
}

export interface CompletedFileStat {
  name: string;
  size: number;
  bytes: number;
  error: string;
  checked: boolean;
}

export interface TransferStats {
  bytes: number;
  total_bytes: number;
  speed: number;
  speed_mbps: number;
  percentage: number;
  eta_seconds?: number | null;
  checks: number;
  transfers: number;
  errors: number;
  fatal_error: boolean;
  retry_error?: boolean;
  transferring: TransferringFile[];
  completed?: CompletedFileStat[];
}

export interface VerificationResult {
  success: boolean;
  hash_type: string;
  matching_files: number;
  missing_on_dst: number;
  differ_count: number;
  error_count: number;
  details: string[];
}

export type TransferMode =
  | "directory_upload"
  | "directory_download"
  | "drive_file_download"
  | "local_file_upload";

export type TransferPhase =
  | "preparing"
  | "transferring"
  | "finalizing"
  | "retrying"
  | "waiting_for_drive"
  | "completed"
  | "failed"
  | "quota_limited";

export type TransferFailureKind =
  | "authentication"
  | "permission_denied"
  | "api_quota"
  | "daily_upload_limit"
  | "network"
  | "disk_space"
  | "integrity_mismatch"
  | "unknown";

export interface StartedTransfer {
  job_id: number;
  mode: TransferMode;
  logical_total_bytes: number;
  logical_file_count: number;
}

export interface CompletedTransfer {
  id: string;
  projectName: string;
  type: "download" | "upload";
  mode?: TransferMode;
  status: "completed" | "failed" | "cancelled" | "quota_limited" | "interrupted";
  error?: string | null;
  source: string;
  destination: string;
  totalBytes: number;
  bytesTransferred?: number;
  fileCount: number;
  timestamp: string;
  verified?: boolean;
  verificationResult?: VerificationResult;
  is_single_file?: boolean;
}

export interface GoogleUserInfo {
  is_authenticated: boolean;
  display_name?: string | null;
  email?: string | null;
  photo_link?: string | null;
  storage_total?: number | null;
  storage_used?: number | null;
}

export interface AppEngineStatus {
  status: string;
  port: number;
  has_gdrive: boolean;
  remotes: string[];
  user_info?: GoogleUserInfo | null;
  engine_version?: string | null;
}

export interface PublicAppSettings {
  oauth_mode: "managed" | "custom";
  is_managed: boolean;
  connected_client_fingerprint?: string | null;
  has_custom_client_id: boolean;
  has_custom_client_secret: boolean;
  has_webhook_url: boolean;
  notify_on_complete: boolean;
}

