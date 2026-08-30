import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Header } from "./components/Header";
import { DownloadView } from "./components/DownloadView";
import { UploadView } from "./components/UploadView";
import { TransfersView } from "./components/TransfersView";
import { HistoryView } from "./components/HistoryView";
import { SettingsView } from "./components/SettingsView";
import {
  AppEngineStatus,
  TransferStats,
  CompletedTransfer,
  TransferMode,
  TransferPhase,
  StartedTransfer,
} from "./types";
import {
  migrateHistoryItem,
  resolveEffectiveSpeed,
  calculateResumeBaseline,
  calculateLogicalProgress,
  deriveTransferPhase,
  CompletedLedgerEntry,
  mergeCompletedEntry,
} from "./utils/transfer";
import "./App.css";

export function App() {
  const [activeTab, setActiveTab] = useState<"download" | "upload" | "transfers" | "history" | "settings">("download");
  const [engineStatus, setEngineStatus] = useState<AppEngineStatus | null>(null);
  const [isDark, setIsDark] = useState<boolean>(() => {
    return localStorage.getItem("balladi_theme") === "dark";
  });

  useEffect(() => {
    if (isDark) {
      document.documentElement.classList.add("dark");
      localStorage.setItem("balladi_theme", "dark");
    } else {
      document.documentElement.classList.remove("dark");
      localStorage.setItem("balladi_theme", "light");
    }
  }, [isDark]);

  // Active Transfer State
  const [activeJobId, setActiveJobId] = useState<number | null>(null);
  const [transferError, setTransferError] = useState<string | null>(null);
  const [projectName, setProjectName] = useState<string>("");
  const [sourcePath, setSourcePath] = useState<string>("");
  const [destinationPath, setDestinationPath] = useState<string>("");
  const [transferType, setTransferType] = useState<"download" | "upload">("download");
  const [transferPhase, setTransferPhase] = useState<TransferPhase>("preparing");
  const [stats, setStats] = useState<TransferStats | null>(null);
  const [isPaused, setIsPaused] = useState<boolean>(false);
  const [bwLimit, setBwLimit] = useState<string>("unlimited");

  // Online/Offline Network Status
  const [isOnline, setIsOnline] = useState<boolean>(navigator.onLine);
  useEffect(() => {
    const handleOnline = () => setIsOnline(true);
    const handleOffline = () => setIsOnline(false);
    window.addEventListener("online", handleOnline);
    window.addEventListener("offline", handleOffline);
    return () => {
      window.removeEventListener("online", handleOnline);
      window.removeEventListener("offline", handleOffline);
    };
  }, []);

  const [sessionMetrics, setSessionMetrics] = useState({
    totalBytes: 0,
    transferredBytes: 0,
    completedFiles: 0,
    alreadyOnDiskFiles: 0,
    percentage: 0,
  });

  const [history, setHistory] = useState<CompletedTransfer[]>(() => {
    try {
      const saved = localStorage.getItem("balladi_transfer_history");
      if (!saved) return [];
      const parsed = JSON.parse(saved);
      return Array.isArray(parsed) ? parsed.map(migrateHistoryItem) : [];
    } catch {
      return [];
    }
  });

  // Base metrics for resumed transfers (to ensure cumulative totals across pause/resumes)
  const resumeBaseRef = useRef({ bytes: 0, completedFiles: 0 });
  const speedSampleRef = useRef<{ jobId: number; bytes: number; timeMs: number } | null>(null);
  const completedLedgerRef = useRef<Map<string, CompletedLedgerEntry>>(new Map());
  const logicalTotalBytesRef = useRef<number>(0);
  const logicalFileCountRef = useRef<number>(0);
  const lastMovementMsRef = useRef<number>(Date.now());
  const lastProgressBytesRef = useRef<number>(0);

  const pollIntervalRef = useRef<any>(null);

  // Initialize engine on app launch
  useEffect(() => {
    const init = async () => {
      try {
        const res = await invoke<AppEngineStatus>("init_engine");
        setEngineStatus(res);
      } catch (err) {
        console.error("Failed to initialize engine:", err);
      }
    };
    init();
  }, []);

  // Save history to localStorage
  useEffect(() => {
    localStorage.setItem("balladi_transfer_history", JSON.stringify(history));
  }, [history]);



  const pollGenerationRef = useRef(0);

  // Sequential non-overlapping polling loop for active transfer stats with scoped job id and generation safety
  useEffect(() => {
    if (!activeJobId) {
      if (pollIntervalRef.current) {
        clearTimeout(pollIntervalRef.current);
        pollIntervalRef.current = null;
      }
      return;
    }

    pollGenerationRef.current += 1;
    const currentGen = pollGenerationRef.current;
    let isPolling = false;

    const poll = async () => {
      if (isPolling || pollGenerationRef.current !== currentGen || !activeJobId) return;
      isPolling = true;

      try {
        const rawStats = await invoke<TransferStats>("get_transfer_stats", {
          jobId: activeJobId,
        });
        if (pollGenerationRef.current !== currentGen) return;

        // Byte-delta speed fallback ONLY for zero-speed reporting during disk flushing/buffering
        const now = performance.now();
        const effectiveSpeed = resolveEffectiveSpeed(
          rawStats.speed,
          speedSampleRef.current,
          {
            jobId: activeJobId,
            bytes: rawStats.bytes,
            timeMs: now,
          }
        );
        speedSampleRef.current = {
          jobId: activeJobId,
          bytes: rawStats.bytes,
          timeMs: now,
        };

        const currentStats: TransferStats = {
          ...rawStats,
          speed: effectiveSpeed,
          speed_mbps: (effectiveSpeed * 8) / 1_000_000,
        };

        setStats(currentStats);

        // Accumulate and deduplicate core/transferred ledger using success-dominant merging
        if (rawStats.completed && Array.isArray(rawStats.completed)) {
          for (const c of rawStats.completed) {
            mergeCompletedEntry(completedLedgerRef.current, c.name, {
              size: c.size,
              bytes: c.bytes,
              error: c.error || "",
              checked: c.checked ?? false,
            });
          }
        }

        const targetLogicalTotal = logicalTotalBytesRef.current > 0
          ? logicalTotalBytesRef.current
          : (rawStats.total_bytes || 0);

        const progress = calculateLogicalProgress(
          targetLogicalTotal,
          completedLedgerRef.current,
          rawStats.transferring || []
        );

        if (progress.logicalProgressBytes > lastProgressBytesRef.current) {
          lastMovementMsRef.current = Date.now();
          lastProgressBytesRef.current = progress.logicalProgressBytes;
        }
        const noMovementForThirtySeconds = (Date.now() - lastMovementMsRef.current) >= 30000;

        // Check if job finished
        const jobStatus: any = await invoke("check_job_status", { jobId: activeJobId });

        const phase = deriveTransferPhase({
          jobFinished: Boolean(jobStatus?.finished),
          jobSucceeded: Boolean(jobStatus?.success),
          error: jobStatus?.error ? String(jobStatus.error) : null,
          retryError: rawStats.retry_error,
          failedAttempts: rawStats.errors,
          logicalTotalBytes: targetLogicalTotal,
          logicalProgressBytes: progress.logicalProgressBytes,
          activeTransfersCount: (rawStats.transferring || []).length,
          noMovementForThirtySeconds,
        });
        setTransferPhase(phase);

        setSessionMetrics((previous) => {
          const totalBytes = targetLogicalTotal > 0
            ? targetLogicalTotal
            : Math.max(previous.totalBytes, resumeBaseRef.current.bytes + (currentStats.total_bytes || 0), progress.logicalProgressBytes);

          return {
            totalBytes,
            transferredBytes: progress.logicalProgressBytes,
            completedFiles: progress.committedCount,
            alreadyOnDiskFiles: Math.max(previous.alreadyOnDiskFiles, currentStats.checks || 0),
            percentage: progress.percentage,
          };
        });

        if (jobStatus && jobStatus.finished && pollGenerationRef.current === currentGen) {
          let terminalStatus: "completed" | "failed" | "cancelled" | "quota_limited" = "completed";
          let errorDetail: string | null = null;

          if (jobStatus.error) {
            errorDetail = String(jobStatus.error);
            const errLower = errorDetail.toLowerCase();
            if (errLower.includes("quota") || errLower.includes("user rate limit") || errLower.includes("upload limit")) {
              terminalStatus = "quota_limited";
            } else if (errLower.includes("canceled") || errLower.includes("context canceled") || errLower.includes("stopped")) {
              terminalStatus = "cancelled";
            } else {
              terminalStatus = "failed";
            }
          } else if (jobStatus.success === false) {
            terminalStatus = "failed";
            errorDetail = "Transfer halted or encountered errors.";
          }

          const finalTotalBytes = targetLogicalTotal > 0 ? targetLogicalTotal : progress.logicalProgressBytes;
          const finalFileCount = logicalFileCountRef.current > 0
            ? logicalFileCountRef.current
            : (progress.committedCount || currentStats.transfers || 1);

          // Record accurate terminal state in history using logical metrics (never wire retry bytes)
          const newRecord: CompletedTransfer = {
            id: String(Date.now()),
            projectName: projectName || "Media Project",
            type: transferType,
            mode: activeTransferModeRef.current,
            status: terminalStatus,
            error: errorDetail,
            source: sourcePath,
            destination: destinationPath,
            totalBytes: finalTotalBytes,
            bytesTransferred: progress.logicalProgressBytes,
            fileCount: finalFileCount,
            timestamp: new Date().toLocaleString(),
            verified: false,
            is_single_file: isSingleFileRef.current,
          };

          speedSampleRef.current = null;
          setHistory((prev) => [newRecord, ...prev]);
          setActiveJobId(null);
          setStats(null);
          setIsPaused(false);
          setActiveTab("history");

          // Webhook notification via Rust backend ONLY for confirmed successful transfers
          if (terminalStatus === "completed") {
            invoke("send_completion_webhook", {
              projectName: newRecord.projectName,
              totalBytes: newRecord.totalBytes,
              fileCount: newRecord.fileCount,
            }).catch((wErr) => console.warn("Webhook dispatch failed:", wErr));
          }
          return;
        }
      } catch (err) {
        console.error("Polling error:", err);
      } finally {
        isPolling = false;
        if (pollGenerationRef.current === currentGen && activeJobId) {
          pollIntervalRef.current = setTimeout(poll, 1000) as any;
        }
      }
    };

    pollIntervalRef.current = setTimeout(poll, 100) as any;

    return () => {
      pollGenerationRef.current += 1;
      if (pollIntervalRef.current) {
        clearTimeout(pollIntervalRef.current);
      }
    };
  }, [activeJobId, projectName, transferType, sourcePath, destinationPath]);

  // Track authoritative transfer mode and single-file flag
  const isSingleFileRef = useRef(false);
  const activeTransferModeRef = useRef<TransferMode>("directory_download");

  // Start Transfer Handler
  const handleStartTransfer = async (
    source: string,
    destination: string,
    projName: string,
    isSingleFile?: boolean,
    typeOverride?: "download" | "upload"
  ) => {
    setTransferError(null);
    pollGenerationRef.current += 1;
    if (pollIntervalRef.current) {
      clearTimeout(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }
    isSingleFileRef.current = Boolean(isSingleFile);

    try {
      // Ensure any previous lingering job or throttle is completely cleared
      try {
        await invoke("stop_all_transfers");
      } catch (_) {}

      const resolvedType = typeOverride || (activeTab === "upload" ? "upload" : "download");
      setSourcePath(source);
      setDestinationPath(destination);
      setProjectName(projName);
      setTransferType(resolvedType);
      setIsPaused(false);
      resumeBaseRef.current = { bytes: 0, completedFiles: 0 };
      speedSampleRef.current = null;
      setSessionMetrics({
        totalBytes: 0,
        transferredBytes: 0,
        completedFiles: 0,
        alreadyOnDiskFiles: 0,
        percentage: 0,
      });

      const started = await invoke<StartedTransfer>("start_transfer_job", {
        source,
        destination,
        isSingleFile: isSingleFileRef.current,
      });

      completedLedgerRef.current.clear();
      logicalTotalBytesRef.current = started.logical_total_bytes;
      logicalFileCountRef.current = started.logical_file_count;
      lastMovementMsRef.current = Date.now();
      lastProgressBytesRef.current = 0;
      setTransferPhase("preparing");

      activeTransferModeRef.current = started.mode;
      isSingleFileRef.current =
        started.mode === "drive_file_download" || started.mode === "local_file_upload";
      setActiveJobId(started.job_id);
      setActiveTab("transfers");
    } catch (err: any) {
      console.error("Failed to start transfer:", err);
      const rawMsg = err?.message || String(err);
      if (rawMsg.includes("didn't find section in config file") || rawMsg.includes("gdrive")) {
        setTransferError("Google Drive is not authenticated yet. Please go to the Settings tab and click 'Connect Google Account' first.");
      } else {
        setTransferError(`Could not start transfer: ${rawMsg}`);
      }
    }
  };

  // Pause / Resume Toggle
  const handlePauseToggle = async () => {
    pollGenerationRef.current += 1;
    if (pollIntervalRef.current) {
      clearTimeout(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }
    speedSampleRef.current = null;

    if (isPaused) {
      // Resume from pause: do not overwrite resumeBaseRef here
      setIsPaused(false);
      try {
        await invoke("set_bandwidth_throttle", { limit: bwLimit === "unlimited" ? "off" : bwLimit });
        const started = await invoke<StartedTransfer>("start_transfer_job", {
          source: sourcePath,
          destination: destinationPath,
          isSingleFile: isSingleFileRef.current,
        });
        activeTransferModeRef.current = started.mode;
        isSingleFileRef.current =
          started.mode === "drive_file_download" || started.mode === "local_file_upload";
        setActiveJobId(started.job_id);
      } catch (err) {
        console.error("Failed to resume transfer:", err);
        setTransferError("Could not resume transfer. Network or Google Drive error.");
      }
    } else {
      // Pause: capture durable completed bytes excluding active in-flight partial bytes before stopping
      if (!activeJobId) return;
      resumeBaseRef.current = calculateResumeBaseline(
        sessionMetrics.transferredBytes,
        sessionMetrics.completedFiles,
        stats?.transferring || []
      );

      try {
        setIsPaused(true);
        await invoke("stop_transfer_job", { jobId: activeJobId });
        setActiveJobId(null);
      } catch (err) {
        console.error("Failed to pause transfer:", err);
      }
    }
  };

  // Cancel Transfer Handler
  const handleCancelTransfer = async () => {
    pollGenerationRef.current += 1;
    if (pollIntervalRef.current) {
      clearTimeout(pollIntervalRef.current);
      pollIntervalRef.current = null;
    }
    speedSampleRef.current = null;

    // Record partial progress in History before clearing
    const baseline = calculateResumeBaseline(
      sessionMetrics.transferredBytes,
      sessionMetrics.completedFiles,
      stats?.transferring || []
    );
    const cumulativeBytes = baseline.bytes;
    const cumulativeFiles = baseline.completedFiles;
    if (cumulativeBytes > 0 || cumulativeFiles > 0) {
      const cancelledRecord: CompletedTransfer = {
        id: String(Date.now()),
        projectName: projectName || "Interrupted Media Transfer",
        type: transferType,
        mode: activeTransferModeRef.current,
        status: "cancelled",
        source: sourcePath,
        destination: destinationPath,
        totalBytes: sessionMetrics.totalBytes,
        bytesTransferred: cumulativeBytes,
        fileCount: cumulativeFiles,
        timestamp: new Date().toLocaleString(),
        verified: false,
        is_single_file: isSingleFileRef.current,
      };
      setHistory((prev) => [cancelledRecord, ...prev]);
    }

    try {
      await invoke("stop_all_transfers");
    } catch (err) {
      console.error("Failed stopping transfers:", err);
    }
    resumeBaseRef.current = { bytes: 0, completedFiles: 0 };
    setActiveJobId(null);
    setStats(null);
    setIsPaused(false);
    setActiveTab("history");
  };

  // Dynamic Bandwidth Throttle
  const handleSetBwLimit = async (limit: string) => {
    setBwLimit(limit);
    try {
      await invoke("set_bandwidth_throttle", { limit: limit === "unlimited" ? "off" : limit });
    } catch (err) {
      console.error("Failed setting bwlimit:", err);
    }
  };

  const handleUpdateHistoryItem = (id: string, updates: Partial<CompletedTransfer>) => {
    setHistory((prev) =>
      prev.map((item) => (item.id === id ? { ...item, ...updates } : item))
    );
  };

  const handleClearHistory = () => {
    setHistory([]);
    localStorage.removeItem("balladi_transfer_history");
  };

  return (
    <div className="min-h-screen bg-slate-50 dark:bg-zinc-950 text-slate-900 dark:text-zinc-100 flex flex-col selection:bg-blue-600 selection:text-white transition-colors">
      <Header
        activeTab={activeTab}
        setActiveTab={setActiveTab}
        engineStatus={engineStatus}
        activeTransferCount={activeJobId ? 1 : 0}
        isDark={isDark}
        setIsDark={setIsDark}
      />

      <main className="flex-1 max-w-6xl w-full mx-auto p-6 md:p-8">
        {transferError && (
          <div className="mb-6 p-4 bg-rose-50 dark:bg-rose-950/60 border border-rose-200 dark:border-rose-800 rounded-2xl flex items-start justify-between gap-3 text-rose-900 dark:text-rose-200 shadow-sm">
            <div className="text-xs space-y-1">
              <p className="font-bold flex items-center gap-1.5">Transfer Failed to Start</p>
              <p className="text-[11px] leading-relaxed text-rose-700 dark:text-rose-300">{transferError}</p>
            </div>
            <button
              onClick={() => setTransferError(null)}
              className="text-xs px-2.5 py-1 bg-rose-100 dark:bg-rose-900/60 hover:bg-rose-200 text-rose-800 dark:text-rose-200 rounded-lg font-medium transition"
            >
              Dismiss
            </button>
          </div>
        )}

        {activeTab === "download" && (
          <DownloadView
            onStartTransfer={handleStartTransfer}
            engineStatus={engineStatus}
            onGoToSettings={() => setActiveTab("settings")}
          />
        )}

        {activeTab === "upload" && (
          <UploadView onStartTransfer={handleStartTransfer} />
        )}

        {activeTab === "transfers" && (
          <TransfersView
            activeJobId={activeJobId}
            transferType={transferType}
            phase={transferPhase}
            stats={stats}
            sessionMetrics={sessionMetrics}
            isOnline={isOnline}
            projectName={projectName}
            sourcePath={sourcePath}
            destinationPath={destinationPath}
            isPaused={isPaused}
            onPauseToggle={handlePauseToggle}
            onCancelTransfer={handleCancelTransfer}
            onSetBwLimit={handleSetBwLimit}
            currentBwLimit={bwLimit}
          />
        )}

        {activeTab === "history" && (
          <HistoryView
            history={history}
            onClearHistory={handleClearHistory}
            onResumeTransfer={handleStartTransfer}
            onUpdateHistoryItem={handleUpdateHistoryItem}
          />
        )}

        {activeTab === "settings" && (
          <SettingsView
            engineStatus={engineStatus}
            onRefreshEngineStatus={async () => {
              try {
                const res = await invoke<AppEngineStatus>("init_engine");
                setEngineStatus(res);
              } catch (err) {
                console.error("Failed refreshing engine status:", err);
              }
            }}
          />
        )}
      </main>

      {/* Footer */}
      <footer className="border-t border-slate-200 dark:border-zinc-900 py-3 px-6 text-center text-[11px] text-slate-400 dark:text-zinc-600 font-mono">
        Balladi Drive • High-Reliability Google Drive Media Transfer Engine
      </footer>
    </div>
  );
}

export default App;
