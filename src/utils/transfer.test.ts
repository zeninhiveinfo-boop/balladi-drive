import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  migrateHistoryItem,
  resolveEffectiveSpeed,
  calculateResumeBaseline,
  calculateLogicalProgress,
  classifyFailureKind,
  deriveTransferPhase,
  mergeCompletedEntry,
  CompletedLedgerEntry,
} from "./transfer.ts";

describe("transfer utilities", () => {
  it("migrates legacy history records without mode or with mode 'directory'", () => {
    assert.equal(
      migrateHistoryItem({ type: "upload", mode: "directory" }).mode,
      "directory_upload"
    );

    assert.equal(
      migrateHistoryItem({ type: "download", mode: "directory" }).mode,
      "directory_download"
    );

    assert.equal(
      migrateHistoryItem({ type: "upload", is_single_file: true }).mode,
      "local_file_upload"
    );

    assert.equal(
      migrateHistoryItem({ type: "download", is_single_file: true }).mode,
      "drive_file_download"
    );

    // Preserves modern mode
    assert.equal(
      migrateHistoryItem({ type: "upload", mode: "directory_upload" }).mode,
      "directory_upload"
    );
  });

  it("prioritizes authoritative rclone speed and falls back to byte-delta only when zero", () => {
    const previous = { jobId: 1, bytes: 10_000_000, timeMs: 1000 };
    const current = { jobId: 1, bytes: 20_000_000, timeMs: 2000 };

    // Valid positive rclone speed must always win
    assert.equal(resolveEffectiveSpeed(25_000_000, previous, current), 25_000_000);

    // Delta fallback is used when rclone reports 0
    const deltaSpeed = resolveEffectiveSpeed(0, previous, current);
    assert.equal(deltaSpeed, 10_000_000); // 10MB in 1000ms = 10MB/s

    // Returns 0 if no prior sample
    assert.equal(resolveEffectiveSpeed(0, null, current), 0);
  });

  it("calculates durable resume baseline by deducting active in-flight bytes", () => {
    const transferring = [
      { bytes: 50_000_000, size: 100_000_000 },
      { bytes: 20_000_000, size: 50_000_000 },
    ];

    const baseline = calculateResumeBaseline(150_000_000, 3, transferring);
    // inFlight = 50MB + 20MB = 70MB
    // baseline = 150MB - 70MB = 80MB
    assert.equal(baseline.bytes, 80_000_000);
    assert.equal(baseline.completedFiles, 3);
  });

  it("calculates logical progress without inflation from retry traffic", () => {
    const completedLedger = new Map<string, { size: number; bytes: number; error: string }>();
    completedLedger.set("clip1.mov", { size: 500_000_000, bytes: 500_000_000, error: "" });
    completedLedger.set("clip2.mov", { size: 500_000_000, bytes: 500_000_000, error: "" });

    // File 3 is currently transferring
    const transferring = [
      { name: "clip3.mov", bytes: 41_078_181, size: 41_078_181 },
    ];

    const logicalTotal = 1_041_078_181;
    const progress = calculateLogicalProgress(logicalTotal, completedLedger, transferring);

    assert.equal(progress.committedBytes, 1_000_000_000);
    assert.equal(progress.activeBytes, 41_078_181);
    assert.equal(progress.logicalProgressBytes, 1_041_078_181);
    assert.equal(progress.percentage, 100);
    assert.equal(progress.committedCount, 2);
  });

  it("credits checked already-present files with full logical size and count", () => {
    const completedLedger = new Map<string, { size: number; bytes: number; error: string; checked?: boolean }>();
    // Checked file from rclone has bytes: 0, checked: true, error: ""
    completedLedger.set("clip1.mov", { size: 500_000_000, bytes: 0, error: "", checked: true });
    // Normal file
    completedLedger.set("clip2.mov", { size: 500_000_000, bytes: 500_000_000, error: "", checked: false });

    const logicalTotal = 1_000_000_000;
    const progress = calculateLogicalProgress(logicalTotal, completedLedger, []);

    assert.equal(progress.committedBytes, 1_000_000_000);
    assert.equal(progress.committedCount, 2);
    assert.equal(progress.percentage, 100);
  });

  it("prevents failed retry attempts from overwriting earlier successful files in ledger using mergeCompletedEntry", () => {
    const ledger = new Map<string, CompletedLedgerEntry>();

    // Initial successful file
    mergeCompletedEntry(ledger, "scene1.mov", { size: 200_000_000, bytes: 200_000_000, error: "", checked: false });
    assert.equal(ledger.get("scene1.mov")?.error, "");
    assert.equal(ledger.get("scene1.mov")?.bytes, 200_000_000);

    // Subsequent failed attempt on the same file must NOT overwrite the successful entry
    mergeCompletedEntry(ledger, "scene1.mov", { size: 200_000_000, bytes: 100_000_000, error: "403 RATE_LIMIT_EXCEEDED", checked: false });
    assert.equal(ledger.get("scene1.mov")?.error, "");
    assert.equal(ledger.get("scene1.mov")?.bytes, 200_000_000);

    // But a successful attempt DOES overwrite a previous failed entry
    mergeCompletedEntry(ledger, "scene2.mov", { size: 300_000_000, bytes: 50_000_000, error: "network timeout", checked: false });
    assert.equal(ledger.get("scene2.mov")?.error, "network timeout");

    mergeCompletedEntry(ledger, "scene2.mov", { size: 300_000_000, bytes: 300_000_000, error: "", checked: false });
    assert.equal(ledger.get("scene2.mov")?.error, "");
    assert.equal(ledger.get("scene2.mov")?.bytes, 300_000_000);
  });

  it("classifies errors accurately: userRateLimitExceeded -> api_quota, 750GB -> daily_upload_limit", () => {
    assert.equal(
      classifyFailureKind("403 RATE_LIMIT_EXCEEDED Quota exceeded for quota metric 'Queries'"),
      "api_quota"
    );
    assert.equal(
      classifyFailureKind("userRateLimitExceeded: User Rate Limit Exceeded"),
      "api_quota"
    );
    assert.equal(
      classifyFailureKind("user rate limit exceeded"),
      "api_quota"
    );
    assert.equal(
      classifyFailureKind("750GB upload limit reached"),
      "daily_upload_limit"
    );
    assert.equal(
      classifyFailureKind("dailyLimitExceeded: Drive storage limit reached"),
      "daily_upload_limit"
    );
    assert.equal(
      classifyFailureKind("daily upload limit exceeded"),
      "daily_upload_limit"
    );
    assert.equal(
      classifyFailureKind("oauth token invalid_grant unauthorized"),
      "authentication"
    );
    assert.equal(
      classifyFailureKind("ENOSPC: no space left on device"),
      "disk_space"
    );
    assert.equal(
      classifyFailureKind("MD5 hash mismatch corruption detected"),
      "integrity_mismatch"
    );
    assert.equal(
      classifyFailureKind("connection reset by peer network timeout"),
      "network"
    );
  });

  it("derives truthful transfer phases across lifecycle states", () => {
    // 1. Preparing
    assert.equal(
      deriveTransferPhase({
        jobFinished: false,
        jobSucceeded: false,
        logicalTotalBytes: 0,
        logicalProgressBytes: 0,
        activeTransfersCount: 0,
        noMovementForThirtySeconds: false,
      }),
      "preparing"
    );

    // 2. Active transferring
    assert.equal(
      deriveTransferPhase({
        jobFinished: false,
        jobSucceeded: false,
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 500_000,
        activeTransfersCount: 2,
        noMovementForThirtySeconds: false,
      }),
      "transferring"
    );

    // 3. Finalizing (100% transmitted but job not yet finished)
    assert.equal(
      deriveTransferPhase({
        jobFinished: false,
        jobSucceeded: false,
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 1_000_000,
        activeTransfersCount: 1,
        noMovementForThirtySeconds: false,
      }),
      "finalizing"
    );

    // 4. Retrying
    assert.equal(
      deriveTransferPhase({
        jobFinished: false,
        jobSucceeded: false,
        retryError: true,
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 500_000,
        activeTransfersCount: 1,
        noMovementForThirtySeconds: false,
      }),
      "retrying"
    );

    // 5. Waiting for drive
    assert.equal(
      deriveTransferPhase({
        jobFinished: false,
        jobSucceeded: false,
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 500_000,
        activeTransfersCount: 1,
        noMovementForThirtySeconds: true,
      }),
      "waiting_for_drive"
    );

    // 6. Quota limited
    assert.equal(
      deriveTransferPhase({
        jobFinished: false,
        jobSucceeded: false,
        error: "403 RATE_LIMIT_EXCEEDED",
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 500_000,
        activeTransfersCount: 0,
        noMovementForThirtySeconds: false,
      }),
      "quota_limited"
    );

    // 7. Completed (requires finished === true && success === true)
    assert.equal(
      deriveTransferPhase({
        jobFinished: true,
        jobSucceeded: true,
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 1_000_000,
        activeTransfersCount: 0,
        noMovementForThirtySeconds: false,
      }),
      "completed"
    );

    // 8. Failed
    assert.equal(
      deriveTransferPhase({
        jobFinished: true,
        jobSucceeded: false,
        logicalTotalBytes: 1_000_000,
        logicalProgressBytes: 500_000,
        activeTransfersCount: 0,
        noMovementForThirtySeconds: false,
      }),
      "failed"
    );
  });
});
