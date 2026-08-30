import { CompletedTransfer, TransferFailureKind, TransferMode, TransferPhase } from "../types";

/**
 * Migrates legacy history records where mode was "directory" or unset
 * to the explicit direction-aware TransferMode ("directory_upload" | "directory_download").
 */
export function migrateHistoryItem(item: any): CompletedTransfer {
  let mode: TransferMode = item.mode;

  if (mode === ("directory" as any) || !mode) {
    if (item.is_single_file) {
      mode = item.type === "upload" ? "local_file_upload" : "drive_file_download";
    } else {
      mode = item.type === "upload" ? "directory_upload" : "directory_download";
    }
  }

  return {
    ...item,
    mode,
  };
}

/**
 * Resolves the effective transfer speed in bytes/sec.
 * Gives precedence to authoritative rclone speed samples (> 0),
 * falling back to byte-delta calculation ONLY when rclone reports 0 (e.g. during buffer flushing).
 */
export function resolveEffectiveSpeed(
  rcloneSpeed: number,
  previous: { jobId: number; bytes: number; timeMs: number } | null,
  current: { jobId: number; bytes: number; timeMs: number }
): number {
  let effectiveSpeed = rcloneSpeed;

  if (
    effectiveSpeed <= 0 &&
    previous?.jobId === current.jobId &&
    current.bytes >= previous.bytes &&
    current.timeMs > previous.timeMs
  ) {
    const sampledSpeed =
      ((current.bytes - previous.bytes) * 1000) / (current.timeMs - previous.timeMs);

    if (Number.isFinite(sampledSpeed) && sampledSpeed > 0) {
      effectiveSpeed = sampledSpeed;
    }
  }

  return Math.max(0, effectiveSpeed);
}

/**
 * Computes durable resume baseline by deducting active in-flight partial bytes.
 */
export function calculateResumeBaseline(
  transferredBytes: number,
  completedFiles: number,
  transferringList: Array<{ bytes: number; size: number }> = []
): { bytes: number; completedFiles: number } {
  const inFlightBytes = transferringList.reduce(
    (sum, file) => sum + Math.min(file.bytes, file.size),
    0
  );

  return {
    bytes: Math.max(0, transferredBytes - inFlightBytes),
    completedFiles,
  };
}

export interface CompletedLedgerEntry {
  size: number;
  bytes: number;
  error: string;
  checked?: boolean;
}

/**
 * Merges an incoming file stat into the completed ledger using success-dominant rules.
 * An earlier successful entry cannot be overwritten by a subsequent transient failure/retry.
 */
export function mergeCompletedEntry(
  ledger: Map<string, CompletedLedgerEntry>,
  name: string,
  incoming: CompletedLedgerEntry
): void {
  const previous = ledger.get(name);
  const previousSucceeded =
    previous &&
    !previous.error &&
    (previous.checked || previous.bytes >= previous.size);

  const incomingSucceeded =
    !incoming.error &&
    (incoming.checked || incoming.bytes >= incoming.size);

  if (!previousSucceeded || incomingSucceeded) {
    ledger.set(name, incoming);
  }
}

/**
 * Calculates accurate logical progress by combining durable committed file ledger
 * with in-flight uncommitted bytes (capped at each file's logical size).
 * Ensures progress never exceeds logicalTotalBytes or changes due to wire retries.
 * Properly credits skipped/already-present files ({ checked: true, bytes: 0 }).
 */
export function calculateLogicalProgress(
  logicalTotalBytes: number,
  completedLedger: Map<string, CompletedLedgerEntry>,
  currentTransfers: Array<{ name: string; bytes: number; size: number }> = []
): {
  committedBytes: number;
  activeBytes: number;
  logicalProgressBytes: number;
  percentage: number;
  committedCount: number;
} {
  let committedBytes = 0;
  let committedCount = 0;
  const committedNames = new Set<string>();

  for (const [name, stat] of completedLedger.entries()) {
    const successful = !stat.error && (stat.checked || stat.bytes >= stat.size);
    if (successful) {
      committedBytes += stat.size;
      committedCount++;
      committedNames.add(name);
    }
  }

  const activeBytes = currentTransfers
    .filter((file) => !committedNames.has(file.name))
    .reduce((sum, file) => sum + Math.min(file.bytes, file.size), 0);

  const logicalProgressBytes =
    logicalTotalBytes > 0
      ? Math.min(logicalTotalBytes, committedBytes + activeBytes)
      : committedBytes + activeBytes;

  const percentage =
    logicalTotalBytes > 0
      ? Math.min(100, (logicalProgressBytes / logicalTotalBytes) * 100)
      : 0;

  return {
    committedBytes,
    activeBytes,
    logicalProgressBytes,
    percentage,
    committedCount,
  };
}

/**
 * Classifies transfer error messages into structured TransferFailureKind.
 */
export function classifyFailureKind(errorStr: string): TransferFailureKind {
  const lower = errorStr.toLowerCase();
  if (
    lower.includes("dailylimitexceeded") ||
    lower.includes("750gb") ||
    lower.includes("750 gb") ||
    lower.includes("daily upload limit") ||
    lower.includes("upload limit reached") ||
    lower.includes("upload limit exceeded")
  ) {
    return "daily_upload_limit";
  } else if (
    lower.includes("userratelimitexceeded") ||
    lower.includes("user rate limit") ||
    lower.includes("rate_limit_exceeded") ||
    lower.includes("ratelimitexceeded") ||
    lower.includes("queries") ||
    lower.includes("quota exceeded for quota metric") ||
    (lower.includes("403") && lower.includes("quota")) ||
    lower.includes("429")
  ) {
    return "api_quota";
  } else if (
    lower.includes("unauthorized") ||
    lower.includes("token") ||
    lower.includes("oauth") ||
    lower.includes("invalid_grant") ||
    lower.includes("auth")
  ) {
    return "authentication";
  } else if (
    lower.includes("accessdenied") ||
    lower.includes("permission denied") ||
    lower.includes("403 forbidden") ||
    lower.includes("not found")
  ) {
    return "permission_denied";
  } else if (
    lower.includes("no space") ||
    lower.includes("disk full") ||
    lower.includes("enospc")
  ) {
    return "disk_space";
  } else if (
    lower.includes("connection reset") ||
    lower.includes("connection refused") ||
    lower.includes("timeout") ||
    lower.includes("network") ||
    lower.includes("broken pipe")
  ) {
    return "network";
  } else if (
    lower.includes("corrupt") ||
    lower.includes("md5 mismatch") ||
    lower.includes("hash mismatch") ||
    lower.includes("differ")
  ) {
    return "integrity_mismatch";
  }
  return "unknown";
}

/**
 * Derives truthful transfer lifecycle phase.
 */
export function deriveTransferPhase(params: {
  jobFinished: boolean;
  jobSucceeded: boolean;
  error?: string | null;
  retryError?: boolean;
  failedAttempts?: number;
  logicalTotalBytes: number;
  logicalProgressBytes: number;
  activeTransfersCount: number;
  noMovementForThirtySeconds: boolean;
}): TransferPhase {
  const {
    jobFinished,
    jobSucceeded,
    error,
    retryError,
    failedAttempts = 0,
    logicalTotalBytes,
    logicalProgressBytes,
    activeTransfersCount,
    noMovementForThirtySeconds,
  } = params;

  if (jobFinished) {
    return jobSucceeded ? "completed" : "failed";
  }

  if (error) {
    const kind = classifyFailureKind(error);
    if (kind === "api_quota" || kind === "daily_upload_limit") {
      return "quota_limited";
    }
  }

  if (retryError || failedAttempts > 0) {
    return "retrying";
  }

  if (logicalTotalBytes === 0 && activeTransfersCount === 0) {
    return "preparing";
  }

  if (logicalTotalBytes > 0 && logicalProgressBytes >= logicalTotalBytes) {
    return "finalizing";
  }

  if (noMovementForThirtySeconds) {
    return "waiting_for_drive";
  }

  return "transferring";
}
