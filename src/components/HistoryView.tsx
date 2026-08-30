import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  History,
  ShieldCheck,
  CheckCircle2,
  AlertTriangle,
  FolderCheck,
  RotateCw,
  Trash2,
  ArrowUpRight,
  ArrowDownLeft,
  FolderOpen,
  Play,
} from "lucide-react";
import { CompletedTransfer, VerificationResult } from "../types";

interface HistoryViewProps {
  history: CompletedTransfer[];
  onClearHistory: () => void;
  onResumeTransfer?: (
    source: string,
    destination: string,
    projectName: string,
    isSingleFile?: boolean,
    transferType?: "download" | "upload"
  ) => void;
  onUpdateHistoryItem?: (id: string, updates: Partial<CompletedTransfer>) => void;
}

export const HistoryView: React.FC<HistoryViewProps> = ({
  history,
  onClearHistory,
  onResumeTransfer,
  onUpdateHistoryItem,
}) => {
  const [verifyingId, setVerifyingId] = useState<string | null>(null);
  const [verificationModalData, setVerificationModalData] = useState<{
    projectName: string;
    result: VerificationResult;
  } | null>(null);

  const formatBytes = (bytes: number) => {
    if (!bytes || bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  const handleVerify = async (item: CompletedTransfer) => {
    setVerifyingId(item.id);
    try {
      const result = await invoke<VerificationResult>("verify_transfer_integrity", {
        source: item.source,
        destination: item.destination,
        mode: item.mode,
        isSingleFile: item.is_single_file,
      });

      if (onUpdateHistoryItem) {
        onUpdateHistoryItem(item.id, {
          verified: result.success,
          verificationResult: result,
        });
      }

      setVerificationModalData({
        projectName: item.projectName,
        result,
      });
    } catch (err) {
      console.error("Verification failed:", err);
    } finally {
      setVerifyingId(null);
    }
  };

  const handleReveal = async (path: string) => {
    try {
      await invoke("reveal_in_finder", { path });
    } catch (err) {
      console.error("Failed opening folder in Finder:", err);
    }
  };

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-bold text-slate-900 dark:text-white flex items-center gap-2">
            <History className="w-5 h-5 text-blue-600 dark:text-blue-400" />
            Transfer History & Verification
          </h2>
          <p className="text-xs text-slate-500 dark:text-zinc-400 mt-1">
            Review completed and interrupted transfers, inspect files in Finder, or execute bit-for-bit MD5 checksum verification.
          </p>
        </div>

        {history.length > 0 && (
          <button
            onClick={onClearHistory}
            className="px-3 py-1.5 rounded-lg text-xs font-semibold bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-600 dark:text-zinc-400 border border-slate-200 dark:border-zinc-700 transition flex items-center gap-1.5"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Clear Log
          </button>
        )}
      </div>

      {history.length === 0 ? (
        <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-12 text-center space-y-3 shadow-sm">
          <FolderCheck className="w-12 h-12 text-slate-400 dark:text-zinc-600 mx-auto" />
          <h3 className="text-sm font-semibold text-slate-700 dark:text-zinc-300">No Transfers Logged Yet</h3>
          <p className="text-xs text-slate-500 dark:text-zinc-500 max-w-sm mx-auto">
            Once you transfer or stop a project, it will appear here with saved progress, 1-click Finder reveal, and MD5 verification.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {history.map((item) => {
            const isCompleted = item.status === "completed";
            const isFailed = item.status === "failed";
            const isQuota = item.status === "quota_limited";
            const isInterrupted = item.status === "cancelled" || item.status === "interrupted" || !item.status;
            const canResume = isInterrupted || isQuota || isFailed;

            return (
              <div
                key={item.id}
                className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-4 sm:p-5 hover:border-slate-300 dark:hover:border-zinc-700 transition shadow-sm space-y-3"
              >
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                  <div className="flex items-center gap-3 min-w-0">
                    <div
                      className={`w-9 h-9 rounded-xl flex items-center justify-center flex-shrink-0 ${
                        isCompleted
                          ? "bg-emerald-50 text-emerald-600 dark:bg-emerald-950/60 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800/60"
                          : isFailed
                          ? "bg-rose-50 text-rose-600 dark:bg-rose-950/60 dark:text-rose-400 border border-rose-200 dark:border-rose-800/60"
                          : "bg-amber-50 text-amber-600 dark:bg-amber-950/60 dark:text-amber-400 border border-amber-200 dark:border-amber-800/60"
                      }`}
                    >
                      {isCompleted ? (
                        item.type === "download" ? <ArrowDownLeft className="w-4 h-4" /> : <ArrowUpRight className="w-4 h-4" />
                      ) : (
                        <AlertTriangle className="w-4 h-4" />
                      )}
                    </div>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <h4 className="text-xs font-bold text-slate-900 dark:text-white truncate">
                          {item.projectName}
                        </h4>
                        <span
                          className={`px-2 py-0.5 rounded text-[10px] font-semibold border ${
                            isCompleted
                              ? "bg-emerald-50 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-300 border-emerald-200 dark:border-emerald-800/60"
                              : isFailed
                              ? "bg-rose-50 dark:bg-rose-950/60 text-rose-700 dark:text-rose-300 border-rose-200 dark:border-rose-800/60"
                              : isQuota
                              ? "bg-amber-50 dark:bg-amber-950/60 text-amber-700 dark:text-amber-300 border-amber-200 dark:border-amber-800/60"
                              : "bg-slate-100 dark:bg-zinc-800 text-slate-700 dark:text-zinc-300 border-slate-200 dark:border-zinc-700"
                          }`}
                        >
                          {isCompleted
                            ? "Completed"
                            : isFailed
                            ? "Failed"
                            : isQuota
                            ? "Google Upload Quota Exceeded (750GB/day)"
                            : "Interrupted / Partial"}
                        </span>
                        {item.verified && (
                          <span className="px-2 py-0.5 rounded text-[10px] font-bold bg-emerald-100 dark:bg-emerald-900/80 text-emerald-800 dark:text-emerald-200 border border-emerald-300 dark:border-emerald-700 flex items-center gap-1">
                            <ShieldCheck className="w-3 h-3 text-emerald-600 dark:text-emerald-300" />
                            MD5 Verified
                          </span>
                        )}
                      </div>
                      <p className="text-[11px] text-slate-400 dark:text-zinc-500 mt-0.5">{item.timestamp}</p>
                    </div>
                  </div>

                  {/* Transfer details and action buttons */}
                  <div className="flex items-center gap-2 flex-wrap sm:flex-nowrap">
                    <span className="text-xs font-mono text-slate-600 dark:text-zinc-300 bg-slate-50 dark:bg-zinc-950 px-2.5 py-1.5 rounded-xl border border-slate-200 dark:border-zinc-800">
                      {isCompleted ? (
                        <>
                          {formatBytes(item.totalBytes)} ({item.fileCount} files)
                        </>
                      ) : (
                        <>
                          <strong className="text-slate-800 dark:text-zinc-200">{formatBytes(item.bytesTransferred || 0)}</strong>
                          <span className="text-slate-400"> / {formatBytes(item.totalBytes)}</span> ({item.fileCount} {item.type === "upload" ? "files in cloud" : "files on disk"})
                        </>
                      )}
                    </span>

                    {canResume && onResumeTransfer && (
                      <button
                        onClick={() => onResumeTransfer(item.source, item.destination, item.projectName, item.is_single_file, item.type)}
                        className="px-3 py-1.5 rounded-xl text-xs font-semibold bg-blue-600 hover:bg-blue-700 text-white transition flex items-center gap-1.5 shadow-sm"
                        title={item.type === "upload" ? "Resume uploading remaining files for this project" : "Resume downloading remaining files for this project"}
                      >
                        <Play className="w-3.5 h-3.5" />
                        Resume
                      </button>
                    )}

                    <button
                      onClick={() => handleReveal(item.type === "upload" ? item.source : item.destination)}
                      className="px-3 py-1.5 rounded-xl text-xs font-semibold bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-700 dark:text-zinc-200 border border-slate-200 dark:border-zinc-700 transition flex items-center gap-1.5"
                      title={item.type === "upload" ? "Reveal local source files in Finder / Explorer" : "Reveal downloaded files in Finder / Explorer"}
                    >
                      <FolderOpen className="w-3.5 h-3.5 text-slate-500" />
                      {item.type === "upload" ? "Open Source" : "Finder"}
                    </button>

                    <button
                      onClick={() => handleVerify(item)}
                      disabled={verifyingId === item.id}
                      className="px-3 py-1.5 rounded-xl text-xs font-semibold bg-emerald-50 hover:bg-emerald-100 dark:bg-emerald-950/60 dark:hover:bg-emerald-900/80 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800/60 transition flex items-center gap-1.5 disabled:opacity-50"
                    >
                      {verifyingId === item.id ? (
                        <RotateCw className="w-3.5 h-3.5 animate-spin" />
                      ) : (
                        <ShieldCheck className="w-3.5 h-3.5 text-emerald-600 dark:text-emerald-400" />
                      )}
                      Verify MD5
                    </button>
                  </div>
                </div>

                {item.error && (
                  <div className="text-xs text-rose-700 dark:text-rose-300 bg-rose-50 dark:bg-rose-950/40 p-2.5 rounded-xl border border-rose-200 dark:border-rose-900/60 flex items-start gap-2">
                    <AlertTriangle className="w-4 h-4 flex-shrink-0 mt-0.5 text-rose-600 dark:text-rose-400" />
                    <span className="font-mono text-[11px] leading-relaxed break-all">{item.error}</span>
                  </div>
                )}

                <div className="text-[11px] font-mono text-slate-500 dark:text-zinc-400 bg-slate-50 dark:bg-zinc-950 p-2.5 rounded-xl border border-slate-100 dark:border-zinc-900 flex flex-col sm:flex-row sm:items-center justify-between gap-1 truncate">
                  <span className="truncate">Source: {item.source}</span>
                  <span className="truncate sm:ml-4 text-slate-700 dark:text-zinc-300">Target: {item.destination}</span>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Verification Result Modal */}
      {verificationModalData && (
        <div className="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm flex items-center justify-center p-4">
          <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl max-w-lg w-full p-6 space-y-5 shadow-xl">
            <div className="flex items-center gap-3">
              <div
                className={`w-10 h-10 rounded-xl flex items-center justify-center ${
                  verificationModalData.result.success
                    ? "bg-emerald-100 dark:bg-emerald-950/80 text-emerald-600 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800"
                    : "bg-rose-100 dark:bg-rose-950/80 text-rose-600 dark:text-rose-400 border border-rose-200 dark:border-rose-800"
                }`}
              >
                {verificationModalData.result.success ? (
                  <CheckCircle2 className="w-5 h-5" />
                ) : (
                  <AlertTriangle className="w-5 h-5" />
                )}
              </div>
              <div>
                <h3 className="text-sm font-bold text-slate-900 dark:text-white">
                  {verificationModalData.result.success
                    ? "Bit-for-Bit Verification Passed"
                    : !verificationModalData.result.hash_type.toUpperCase().includes("MD5")
                    ? "MD5 Checksum Unavailable on Target Remote"
                    : "Verification Failed — Checksum Differences Detected"}
                </h3>
                <p className="text-xs text-slate-500 dark:text-zinc-400 font-mono">
                  {verificationModalData.projectName} ({verificationModalData.result.hash_type || "MD5"})
                </p>
              </div>
            </div>

            {/* Verification Stats Grid */}
            <div className="grid grid-cols-4 gap-2">
              <div className="bg-slate-50 dark:bg-zinc-950 p-2.5 rounded-xl border border-slate-200 dark:border-zinc-800 text-center">
                <span className="text-[10px] text-slate-500 dark:text-zinc-400 block uppercase font-medium">Matching</span>
                <span className="text-base font-extrabold text-emerald-600 dark:text-emerald-400 font-mono">
                  {verificationModalData.result.matching_files}
                </span>
              </div>
              <div className="bg-slate-50 dark:bg-zinc-950 p-2.5 rounded-xl border border-slate-200 dark:border-zinc-800 text-center">
                <span className="text-[10px] text-slate-500 dark:text-zinc-400 block uppercase font-medium">Differing</span>
                <span className={`text-base font-extrabold font-mono ${verificationModalData.result.differ_count === 0 ? "text-slate-400 dark:text-zinc-500" : "text-rose-600 dark:text-rose-400"}`}>
                  {verificationModalData.result.differ_count}
                </span>
              </div>
              <div className="bg-slate-50 dark:bg-zinc-950 p-2.5 rounded-xl border border-slate-200 dark:border-zinc-800 text-center">
                <span className="text-[10px] text-slate-500 dark:text-zinc-400 block uppercase font-medium">Missing</span>
                <span className={`text-base font-extrabold font-mono ${verificationModalData.result.missing_on_dst === 0 ? "text-slate-400 dark:text-zinc-500" : "text-rose-600 dark:text-rose-400"}`}>
                  {verificationModalData.result.missing_on_dst}
                </span>
              </div>
              <div className="bg-slate-50 dark:bg-zinc-950 p-2.5 rounded-xl border border-slate-200 dark:border-zinc-800 text-center">
                <span className="text-[10px] text-slate-500 dark:text-zinc-400 block uppercase font-medium">Errors</span>
                <span className={`text-base font-extrabold font-mono ${verificationModalData.result.error_count === 0 ? "text-slate-400 dark:text-zinc-500" : "text-rose-600 dark:text-rose-400"}`}>
                  {verificationModalData.result.error_count}
                </span>
              </div>
            </div>

            {verificationModalData.result.details?.length > 0 && (
              <div className="bg-slate-50 dark:bg-zinc-950 p-3 rounded-xl border border-slate-200 dark:border-zinc-800 max-h-36 overflow-y-auto font-mono text-[11px] text-slate-600 dark:text-zinc-400 space-y-1">
                {verificationModalData.result.details.map((d, i) => (
                  <div key={i} className="truncate text-rose-600 dark:text-rose-400">{d}</div>
                ))}
              </div>
            )}

            <div className="flex justify-end pt-2">
              <button
                onClick={() => setVerificationModalData(null)}
                className="px-4 py-2 rounded-xl text-xs font-semibold bg-blue-600 hover:bg-blue-700 text-white transition shadow-sm"
              >
                Close Summary
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
