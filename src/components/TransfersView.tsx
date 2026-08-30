import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Pause,
  Play,
  XCircle,
  Clock,
  Gauge,
  HardDrive,
  FileVideo,
  FileText,
  FileImage,
  CheckCircle2,
  Sliders,
  ShieldAlert,
  AlertTriangle,
  Cloud,
  FolderDown,
  ArrowRight,
  WifiOff,
} from "lucide-react";
import { TransferStats, StorageInfo, TransferPhase } from "../types";

export interface SessionMetrics {
  totalBytes: number;
  transferredBytes: number;
  completedFiles: number;
  alreadyOnDiskFiles: number;
  percentage: number;
}

interface TransfersViewProps {
  activeJobId: number | null;
  transferType: "download" | "upload";
  phase?: TransferPhase;
  stats: TransferStats | null;
  sessionMetrics: SessionMetrics;
  isOnline: boolean;
  projectName: string;
  sourcePath: string;
  destinationPath: string;
  isPaused: boolean;
  onPauseToggle: () => void;
  onCancelTransfer: () => void;
  onSetBwLimit: (limit: string) => void;
  currentBwLimit: string;
}

export const TransfersView: React.FC<TransfersViewProps> = ({
  activeJobId,
  transferType,
  phase,
  stats,
  sessionMetrics,
  isOnline,
  projectName,
  sourcePath,
  destinationPath,
  isPaused,
  onPauseToggle,
  onCancelTransfer,
  onSetBwLimit,
  currentBwLimit,
}) => {
  const [showCancelModal, setShowCancelModal] = useState(false);
  const [storageInfo, setStorageInfo] = useState<StorageInfo | null>(null);

  const isUpload = transferType === "upload";

  // Check remaining storage on the destination disk ONLY for local downloads
  useEffect(() => {
    setStorageInfo(null);
    if (transferType !== "download" || !destinationPath) return;

    invoke<StorageInfo>("check_storage_info", { path: destinationPath })
      .then(setStorageInfo)
      .catch((err) => console.error("Storage check error:", err));
  }, [destinationPath, transferType]);

  const activeStreams = stats?.transferring || [];

  const formatBytes = (bytes: number) => {
    if (!bytes || bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  const formatSecondsToEta = (secs: number) => {
    if (secs <= 0) return "Calculating...";
    // Protect against unrealistic ETA numbers (e.g. 652177h) caused by momentary 0-speed pauses
    if (secs > 172800) return "Calculating...";
    const mins = Math.floor(secs / 60);
    const remSecs = secs % 60;
    if (mins >= 60) {
      const hours = Math.floor(mins / 60);
      const remMins = mins % 60;
      return `${hours}h ${remMins}m remaining`;
    }
    return `${mins}m ${remSecs}s remaining`;
  };

  // Instant resilient ETA computation without render-time side effects
  const displayEta = React.useMemo(() => {
    if (isPaused) return "Paused";
    if (!isOnline) return "Waiting for Internet";

    const activeStreams = stats?.transferring || [];
    const allActiveDone = activeStreams.length > 0 && activeStreams.every((s) => s.percentage >= 99.5 || s.bytes >= s.size);

    if (allActiveDone && (stats?.speed || 0) < 50000) {
      return isUpload ? "Finalizing on Drive..." : "Verifying files...";
    }

    // 1. Direct rclone ETA if available and realistic (< 48 hours)
    if (stats?.eta_seconds && stats.eta_seconds > 0 && stats.eta_seconds < 172800) {
      return formatSecondsToEta(stats.eta_seconds);
    }

    // 2. Instant client-side computation from cumulative session speed
    const total = Math.max(sessionMetrics.totalBytes, stats?.total_bytes || 0);
    const current = Math.max(sessionMetrics.transferredBytes, stats?.bytes || 0);
    const remainingBytes = Math.max(0, total - current);

    if (stats?.speed && stats.speed > 50000 && remainingBytes > 0) {
      const calculatedSecs = Math.round(remainingBytes / stats.speed);
      return formatSecondsToEta(calculatedSecs);
    }

    if (allActiveDone) {
      return isUpload ? "Finalizing on Drive..." : "Verifying files...";
    }

    return "Calculating...";
  }, [isPaused, isOnline, isUpload, stats?.eta_seconds, stats?.total_bytes, stats?.bytes, stats?.speed, stats?.transferring, sessionMetrics.totalBytes, sessionMetrics.transferredBytes]);

  const cleanFileName = (path: string) => {
    const parts = path.split("/");
    const fileName = parts.pop() || path;
    const dirPath = parts.join(" / ");
    return { fileName, dirPath };
  };

  const getFileIcon = (fileName: string) => {
    const lower = fileName.toLowerCase();
    if (lower.endsWith(".mp4") || lower.endsWith(".mov") || lower.endsWith(".mkv") || lower.endsWith(".braw") || lower.endsWith(".ari")) {
      return <FileVideo className="w-4 h-4 text-blue-500 flex-shrink-0" />;
    }
    if (lower.endsWith(".jpg") || lower.endsWith(".png") || lower.endsWith(".cr3") || lower.endsWith(".nef") || lower.endsWith(".arw")) {
      return <FileImage className="w-4 h-4 text-emerald-500 flex-shrink-0" />;
    }
    return <FileText className="w-4 h-4 text-slate-400 flex-shrink-0" />;
  };

  // Resilient Cumulative Session-level Metrics (preserves progress across pauses and resumes)
  const displayTotalBytes = Math.max(
    sessionMetrics.totalBytes,
    stats?.total_bytes || 0
  );

  const displayTransferredBytes = Math.max(
    sessionMetrics.transferredBytes,
    stats?.bytes || 0
  );

  const displayCompletedFiles = Math.max(
    sessionMetrics.completedFiles,
    stats?.transfers || 0
  );

  const displayAlreadyOnDisk = Math.max(
    sessionMetrics.alreadyOnDiskFiles,
    stats?.checks || 0
  );

  const displayPercentage = displayTotalBytes > 0
    ? Math.min(100, Math.max(0, (displayTransferredBytes / displayTotalBytes) * 100))
    : (stats?.percentage || sessionMetrics.percentage || 0);

  const speedMbps = isPaused || !isOnline ? 0 : (stats?.speed_mbps || 0);
  const speedMBs = isPaused || !isOnline ? "0.0" : (((stats?.speed || 0) / (1024 * 1024)).toFixed(1));

  // Check if remaining transfer size exceeds free disk space ONLY for local downloads
  const remainingBytesNeeded = Math.max(0, displayTotalBytes - displayTransferredBytes);
  const isDiskCriticallyLow =
    transferType === "download" &&
    storageInfo !== null &&
    storageInfo.free_bytes < remainingBytesNeeded;

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      {/* Offline Internet Warning Banner */}
      {!isOnline && (
        <div className="bg-amber-50 dark:bg-amber-950/70 border-2 border-amber-400 dark:border-amber-600 rounded-2xl p-4 shadow-sm flex items-center gap-3">
          <WifiOff className="w-5 h-5 text-amber-600 dark:text-amber-400 flex-shrink-0 animate-pulse" />
          <div className="space-y-0.5 flex-1">
            <h4 className="text-xs font-bold text-amber-900 dark:text-amber-200 uppercase tracking-wider">
              Network Connection Lost
            </h4>
            <p className="text-xs text-amber-800 dark:text-amber-300">
              Wi-Fi or internet connection is offline. Transfer is standing by and will automatically resume once connection returns. Files on disk are safe.
            </p>
          </div>
        </div>
      )}

      {/* Critical Storage Warning Banner */}
      {isDiskCriticallyLow && (
        <div className="bg-rose-50 dark:bg-rose-950/70 border-2 border-rose-400 dark:border-rose-600 rounded-2xl p-5 shadow-sm space-y-3">
          <div className="flex items-start gap-3">
            <AlertTriangle className="w-5 h-5 text-rose-600 dark:text-rose-400 flex-shrink-0 mt-0.5" />
            <div className="space-y-1.5 flex-1">
              <h4 className="text-xs font-bold uppercase tracking-wider text-rose-900 dark:text-rose-200">
                Warning: Destination Drive Has Insufficient Space
              </h4>
              <p className="text-xs text-rose-800 dark:text-rose-300 leading-relaxed">
                This project requires <strong>{formatBytes(displayTotalBytes)}</strong>, but the target drive only has <strong>{formatBytes(storageInfo.free_bytes)}</strong> available.
                If this transfer continues to your internal Mac disk, macOS will run out of storage and halt the download.
              </p>
              <div className="pt-2 flex items-center gap-2 flex-wrap">
                <button
                  onClick={onPauseToggle}
                  className="px-3.5 py-1.5 bg-rose-600 hover:bg-rose-700 text-white font-semibold text-xs rounded-xl transition shadow-sm"
                >
                  {isPaused ? "Resume Download" : "Pause Download Now"}
                </button>
                <button
                  onClick={() => setShowCancelModal(true)}
                  className="px-3.5 py-1.5 bg-white dark:bg-zinc-800 border border-rose-300 dark:border-rose-700 text-rose-900 dark:text-rose-200 font-semibold text-xs rounded-xl transition hover:bg-rose-100 dark:hover:bg-zinc-700 shadow-sm"
                >
                  Cancel & Switch to External SSD
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Active Transfer Header Card */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 shadow-sm space-y-6">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="space-y-1.5">
            <div className="flex items-center gap-2.5">
              <span className="relative flex h-2.5 w-2.5">
                <span className={`animate-ping absolute inline-flex h-full w-full rounded-full opacity-75 ${
                  isPaused || !isOnline ? "bg-amber-400" : "bg-blue-400"
                }`}></span>
                <span className={`relative inline-flex rounded-full h-2.5 w-2.5 ${
                  isPaused || !isOnline ? "bg-amber-500" : "bg-blue-600"
                }`}></span>
              </span>
              <h2 className="text-base font-bold text-slate-900 dark:text-white tracking-tight">
                {projectName || "Active Media Transfer"}
              </h2>
              <span className="px-2.5 py-0.5 rounded-full text-[10px] font-mono bg-slate-100 dark:bg-zinc-800 text-slate-600 dark:text-zinc-300 border border-slate-200 dark:border-zinc-700 font-semibold">
                {isPaused ? "Paused" : !isOnline ? "Offline" : `Job #${activeJobId || "Active"}`}
              </span>
              {phase && (
                <span
                  className={`px-2.5 py-0.5 rounded-full text-[11px] font-semibold border flex items-center gap-1 ${
                    phase === "preparing"
                      ? "bg-amber-50 dark:bg-amber-950/60 text-amber-800 dark:text-amber-300 border-amber-200 dark:border-amber-800"
                      : phase === "finalizing"
                      ? "bg-purple-50 dark:bg-purple-950/60 text-purple-800 dark:text-purple-300 border-purple-200 dark:border-purple-800"
                      : phase === "retrying"
                      ? "bg-orange-50 dark:bg-orange-950/60 text-orange-800 dark:text-orange-300 border-orange-200 dark:border-orange-800 animate-pulse"
                      : phase === "waiting_for_drive"
                      ? "bg-yellow-50 dark:bg-yellow-950/60 text-yellow-800 dark:text-yellow-300 border-yellow-200 dark:border-yellow-800"
                      : phase === "quota_limited"
                      ? "bg-rose-50 dark:bg-rose-950/60 text-rose-800 dark:text-rose-300 border-rose-200 dark:border-rose-800"
                      : phase === "completed"
                      ? "bg-emerald-50 dark:bg-emerald-950/60 text-emerald-800 dark:text-emerald-300 border-emerald-200 dark:border-emerald-800"
                      : "bg-blue-50 dark:bg-blue-950/60 text-blue-800 dark:text-blue-300 border-blue-200 dark:border-blue-800"
                  }`}
                >
                  {phase === "preparing" && "Preparing Google Drive folders…"}
                  {phase === "transferring" && (isUpload ? "Uploading to Google Drive…" : "Downloading from Google Drive…")}
                  {phase === "finalizing" && "Finalizing files on Google Drive…"}
                  {phase === "retrying" && "Retrying failed Drive request…"}
                  {phase === "waiting_for_drive" && "Waiting for Google Drive API…"}
                  {phase === "quota_limited" && "Google API quota exceeded"}
                  {phase === "completed" && "Completed"}
                  {phase === "failed" && "Failed"}
                </span>
              )}
            </div>

            {/* Clean Source and Destination labels */}
            <div className="text-xs text-slate-600 dark:text-zinc-400 flex items-center gap-2 flex-wrap">
              {isUpload ? (
                <>
                  <span
                    title={`Local Source: ${sourcePath}`}
                    className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-slate-100 dark:bg-zinc-800 border border-slate-200 dark:border-zinc-700 text-slate-700 dark:text-zinc-300 font-mono text-[11px] truncate max-w-[260px]"
                  >
                    <FolderDown className="w-3.5 h-3.5 text-slate-500" />
                    {sourcePath}
                  </span>
                  <ArrowRight className="w-3 h-3 text-slate-400 dark:text-zinc-600 flex-shrink-0" />
                  <span
                    title={`Google Drive Destination: ${destinationPath}`}
                    className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 text-blue-700 dark:text-blue-300 font-medium cursor-help truncate max-w-[260px]"
                  >
                    <Cloud className="w-3.5 h-3.5" />
                    Google Drive: {destinationPath}
                  </span>
                </>
              ) : (
                <>
                  <span
                    title={`Source: ${sourcePath}`}
                    className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-blue-50 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 text-blue-700 dark:text-blue-300 font-medium cursor-help"
                  >
                    <Cloud className="w-3.5 h-3.5" />
                    Google Drive
                  </span>
                  <ArrowRight className="w-3 h-3 text-slate-400 dark:text-zinc-600 flex-shrink-0" />
                  <span
                    title={`Destination: ${destinationPath}`}
                    className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-slate-100 dark:bg-zinc-800 border border-slate-200 dark:border-zinc-700 text-slate-700 dark:text-zinc-300 font-mono text-[11px] truncate max-w-sm"
                  >
                    <FolderDown className="w-3.5 h-3.5 text-slate-500" />
                    {destinationPath}
                  </span>
                </>
              )}
            </div>
          </div>

          {/* Transfer Controls */}
          <div className="flex items-center gap-2">
            <button
              onClick={onPauseToggle}
              className={`px-4 py-2 rounded-xl text-xs font-semibold border transition flex items-center gap-1.5 shadow-sm ${
                isPaused
                  ? "bg-blue-600 hover:bg-blue-700 text-white border-blue-600"
                  : "bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-700 dark:text-zinc-200 border-slate-200 dark:border-zinc-700"
              }`}
            >
              {isPaused ? <Play className="w-3.5 h-3.5" /> : <Pause className="w-3.5 h-3.5" />}
              {isPaused ? "Resume" : "Pause"}
            </button>
            <button
              onClick={() => setShowCancelModal(true)}
              className="px-4 py-2 bg-rose-50 hover:bg-rose-100 dark:bg-rose-950/40 dark:hover:bg-rose-900/60 text-rose-700 dark:text-rose-300 border border-rose-200 dark:border-rose-800/60 rounded-xl text-xs font-semibold transition flex items-center gap-1.5 shadow-sm"
            >
              <XCircle className="w-3.5 h-3.5 text-rose-500 dark:text-rose-400" />
              Cancel
            </button>
          </div>
        </div>

        {/* Progress Bar & Percentage */}
        <div className="space-y-2.5">
          <div className="flex justify-between items-baseline">
            <div className="flex items-baseline gap-2">
              <span className="text-3xl font-extrabold text-slate-900 dark:text-white tracking-tight font-mono">
                {displayPercentage.toFixed(1)}%
              </span>
              <span className="text-xs text-slate-400 dark:text-zinc-500">
                {isPaused ? "paused" : !isOnline ? "waiting for network" : isUpload ? "uploaded" : "transferred"}
              </span>
            </div>
            <span className="text-xs text-slate-600 dark:text-zinc-400 font-mono font-medium">
              {formatBytes(displayTransferredBytes)} of {formatBytes(displayTotalBytes)}
            </span>
          </div>

          <div className="w-full h-3 rounded-full bg-slate-100 dark:bg-zinc-950 border border-slate-200 dark:border-zinc-800 overflow-hidden p-0.5">
            <div
              className={`h-full rounded-full transition-all duration-300 ${
                isPaused || !isOnline ? "bg-amber-500" : "bg-blue-600 dark:bg-blue-500"
              }`}
              style={{ width: `${Math.min(100, Math.max(displayPercentage > 0 ? 1 : 0, displayPercentage))}%` }}
            />
          </div>
        </div>

        {/* Metrics Grid */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <div className="bg-slate-50 dark:bg-zinc-950/60 border border-slate-200 dark:border-zinc-800 rounded-xl p-3.5">
            <span className="text-[11px] text-slate-500 dark:text-zinc-500 block font-medium flex items-center gap-1">
              <Gauge className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" /> Transfer Speed
            </span>
            <span className="text-sm font-bold text-slate-900 dark:text-zinc-100 mt-1 block font-mono">
              {speedMbps.toFixed(1)} <span className="text-xs text-slate-500 font-normal">Mbps</span>
            </span>
            <span className="text-[10px] text-slate-400 dark:text-zinc-500 block">({speedMBs} MB/s)</span>
          </div>

          <div className="bg-slate-50 dark:bg-zinc-950/60 border border-slate-200 dark:border-zinc-800 rounded-xl p-3.5">
            <span className="text-[11px] text-slate-500 dark:text-zinc-500 block font-medium flex items-center gap-1">
              <Clock className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" /> Remaining Time
            </span>
            <span className="text-sm font-bold text-slate-900 dark:text-zinc-100 mt-1 block font-medium">
              {displayEta}
            </span>
          </div>

          <div className="bg-slate-50 dark:bg-zinc-950/60 border border-slate-200 dark:border-zinc-800 rounded-xl p-3.5">
            <span className="text-[11px] text-slate-500 dark:text-zinc-500 block font-medium flex items-center gap-1">
              <HardDrive className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" /> Completed Files
            </span>
            <span className="text-sm font-bold text-slate-900 dark:text-zinc-100 mt-1 block font-mono">
              {displayCompletedFiles} files done
            </span>
          </div>

          <div
            title={
              isUpload
                ? "Files that were already finished in Google Drive before the pause or Wi-Fi reconnect. They were confirmed identical and skipped so they are not uploaded twice."
                : "Files that were already finished on your drive before the pause or Wi-Fi reconnect. They were confirmed to be identical and skipped so they are not downloaded twice."
            }
            className="bg-slate-50 dark:bg-zinc-950/60 border border-slate-200 dark:border-zinc-800 rounded-xl p-3.5 cursor-help"
          >
            <span className="text-[11px] text-slate-500 dark:text-zinc-500 block font-medium flex items-center gap-1">
              <CheckCircle2 className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" /> {isUpload ? "Already in Cloud" : "Already on Disk"}
            </span>
            <span className="text-sm font-bold text-slate-900 dark:text-zinc-100 mt-1 block font-mono">
              {displayAlreadyOnDisk} <span className="text-xs text-slate-500 font-normal">skipped</span>
            </span>
            <span className="text-[10px] text-slate-400 dark:text-zinc-500 block">
              {isUpload ? "(No re-upload needed)" : "(No re-download needed)"}
            </span>
          </div>
        </div>

        {/* Live Bandwidth Throttle Controls */}
        <div className="bg-slate-50 dark:bg-zinc-950/70 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-slate-700 dark:text-zinc-300">
            <Sliders className="w-4 h-4 text-blue-600 dark:text-blue-400" />
            <span>Speed Limit:</span>
            <span className="text-slate-400 dark:text-zinc-500 font-normal font-mono">({currentBwLimit || "Unlimited"})</span>
          </div>

          <div className="flex items-center gap-1.5 flex-wrap">
            {["unlimited", "300M", "100M", "50M", "20M"].map((rate) => (
              <button
                key={rate}
                onClick={() => onSetBwLimit(rate)}
                className={`px-3 py-1 rounded-lg text-xs font-mono font-medium transition ${
                  (currentBwLimit === rate || (rate === "unlimited" && !currentBwLimit))
                    ? "bg-blue-600 text-white shadow-sm"
                    : "bg-white dark:bg-zinc-900 text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-zinc-200 border border-slate-200 dark:border-zinc-800"
                }`}
              >
                {rate === "unlimited" ? "Full Speed" : rate}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Currently Streaming Files Table */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-4 shadow-sm">
        <div className="flex items-center justify-between">
          <h3 className="text-xs font-bold uppercase tracking-wider text-slate-700 dark:text-zinc-300 flex items-center gap-2">
            <FileVideo className="w-4 h-4 text-blue-600 dark:text-blue-400" />
            Active File Streams ({activeStreams.length})
            {isPaused && (
              <span className="text-[10px] lowercase text-amber-600 dark:text-amber-400 font-semibold px-2 py-0.5 rounded bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800">
                paused
              </span>
            )}
          </h3>
          <span className="text-[11px] text-slate-400 dark:text-zinc-500">
            {isPaused
              ? "Streams on hold"
              : activeStreams.length > 0
              ? isUpload
                ? "Multi-threaded upload chunks"
                : "Multi-threaded parallel chunks"
              : "Queue stand-by"}
          </span>
        </div>

        {activeStreams.length > 0 ? (
          <div className="space-y-2.5">
            {activeStreams.map((file, idx) => {
              const { fileName, dirPath } = cleanFileName(file.name);
              return (
                <div
                  key={idx}
                  className="bg-slate-50 dark:bg-zinc-950/80 border border-slate-200 dark:border-zinc-800 rounded-xl p-3.5 space-y-2.5 hover:border-slate-300 dark:hover:border-zinc-700 transition"
                >
                  <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-1.5">
                    <div className="flex items-center gap-2.5 min-w-0">
                      {getFileIcon(fileName)}
                      <div className="min-w-0">
                        <p className="text-xs font-semibold text-slate-900 dark:text-white truncate">
                          {fileName}
                        </p>
                        {dirPath && (
                          <p className="text-[10px] text-slate-400 dark:text-zinc-500 truncate font-mono">
                            {dirPath}
                          </p>
                        )}
                      </div>
                    </div>

                    <div className="text-right flex-shrink-0 flex items-center gap-2">
                      <span className="text-xs font-mono font-medium text-slate-700 dark:text-zinc-300">
                        {formatBytes(file.bytes)} / {formatBytes(file.size)}
                      </span>
                      <span className="text-[11px] font-mono text-blue-600 dark:text-blue-400 font-bold ml-1">
                        {file.percentage.toFixed(0)}%
                      </span>
                    </div>
                  </div>

                  <div className="w-full h-2 rounded-full bg-slate-200 dark:bg-zinc-800/80 overflow-hidden">
                    <div
                      className="h-full bg-blue-600 dark:bg-blue-500 rounded-full transition-all duration-200"
                      style={{ width: `${Math.min(100, file.percentage)}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="py-8 text-center space-y-2">
            {isPaused ? (
              <>
                <div className="w-10 h-10 rounded-xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center mx-auto text-amber-500">
                  <Pause className="w-5 h-5" />
                </div>
                <p className="text-xs font-bold text-slate-800 dark:text-zinc-200">Transfer Paused</p>
                <p className="text-[11px] text-slate-500 dark:text-zinc-400 max-w-sm mx-auto leading-relaxed">
                  {displayCompletedFiles > 0
                    ? isUpload
                      ? `${displayCompletedFiles} uploaded files are safe in Google Drive. Click Resume to continue uploading the rest of the queue.`
                      : `${displayCompletedFiles} completed files are safe on your disk. Click Resume to continue downloading the rest of the queue.`
                    : isUpload
                    ? "Uploaded files remain safe in Google Drive. Click Resume above to continue."
                    : "Completed files remain safe on disk. Click Resume above to continue."}
                </p>
              </>
            ) : !isOnline ? (
              <>
                <div className="w-10 h-10 rounded-xl bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800/60 flex items-center justify-center mx-auto text-amber-500">
                  <WifiOff className="w-5 h-5 animate-pulse" />
                </div>
                <p className="text-xs font-bold text-slate-800 dark:text-zinc-200">Network Disconnected</p>
                <p className="text-[11px] text-slate-500 dark:text-zinc-400 max-w-sm mx-auto leading-relaxed">
                  Waiting for internet connection to return. {isUpload ? "Upload" : "Download"} will resume automatically.
                </p>
              </>
            ) : (
              <>
                <div className="w-6 h-6 border-2 border-blue-600 border-t-transparent rounded-full animate-spin mx-auto" />
                <p className="text-xs font-semibold text-slate-700 dark:text-zinc-300">
                  {isUpload ? "Scanning Local Project..." : "Connecting to Streams..."}
                </p>
                <p className="text-[11px] text-slate-400 dark:text-zinc-500">
                  {isUpload
                    ? "Scanning local files, verifying cloud destination, and preparing chunk uploads."
                    : "Checking disk, skipping finished files, and loading next batch."}
                </p>
              </>
            )}
          </div>
        )}
      </div>

      {/* Cancel Confirmation Modal */}
      {showCancelModal && (
        <div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm flex items-center justify-center p-4">
          <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl max-w-md w-full p-6 space-y-4 shadow-xl">
            <div className="w-10 h-10 rounded-xl bg-rose-100 dark:bg-rose-950/60 border border-rose-200 dark:border-rose-800/60 flex items-center justify-center text-rose-600 dark:text-rose-400">
              <ShieldAlert className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-slate-900 dark:text-white">Cancel Project Transfer?</h3>
              <p className="text-xs text-slate-500 dark:text-zinc-400 mt-1 leading-relaxed">
                {isUpload
                  ? "Files that have already finished uploading will remain safely in Google Drive and be logged to History. You can resume uploading anytime."
                  : "Files that have already finished downloading will remain safely on your disk and be logged to History. You can plug in an external SSD and resume anytime."}
              </p>
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <button
                onClick={() => setShowCancelModal(false)}
                className="px-4 py-2 rounded-xl text-xs font-semibold bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-700 dark:text-zinc-200 transition"
              >
                Keep Transferring
              </button>
              <button
                onClick={() => {
                  setShowCancelModal(false);
                  onCancelTransfer();
                }}
                className="px-4 py-2 rounded-xl text-xs font-semibold bg-rose-600 hover:bg-rose-700 text-white transition shadow-sm"
              >
                Yes, Stop Transfer
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
