import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Download,
  FolderDown,
  HardDrive,
  Link2,
  CheckCircle2,
  AlertTriangle,
  ShieldCheck,
  Zap,
  RotateCw,
} from "lucide-react";
import { ParsedDriveLink, StorageInfo, AppEngineStatus } from "../types";

interface DownloadViewProps {
  onStartTransfer: (
    source: string,
    destination: string,
    projectName: string,
    isSingleFile?: boolean
  ) => void;
  engineStatus: AppEngineStatus | null;
  onGoToSettings: () => void;
}

export const DownloadView: React.FC<DownloadViewProps> = ({
  onStartTransfer,
  engineStatus,
  onGoToSettings,
}) => {
  const [driveUrl, setDriveUrl] = useState("");
  const [parsedLink, setParsedLink] = useState<ParsedDriveLink | null>(null);
  const [projectName, setProjectName] = useState("Media_Project");
  const [destinationPath, setDestinationPath] = useState("");
  const [storageInfo, setStorageInfo] = useState<StorageInfo | null>(null);
  const [estimatedSize, setEstimatedSize] = useState<{ count: number; bytes: number } | null>(null);
  const [isCalculatingSize, setIsCalculatingSize] = useState(false);
  const [encloseFolder, setEncloseFolder] = useState(true);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [diskSpaceWarning, setDiskSpaceWarning] = useState<string | null>(null);
  const sizeCalcGenRef = React.useRef(0);

  // Initialize destination folder with user's default Downloads directory on mount
  useEffect(() => {
    invoke<string>("get_default_download_dir")
      .then((dir) => {
        if (dir) {
          setDestinationPath(dir);
          checkStorage(dir);
        }
      })
      .catch((e) => console.error("Failed getting default download dir:", e));
  }, []);

  // Auto-parse URL whenever it changes and pre-calculate size
  useEffect(() => {
    const currentGen = ++sizeCalcGenRef.current;
    setEstimatedSize(null);
    setIsCalculatingSize(false);
    if (!driveUrl.trim()) {
      setParsedLink(null);
      return;
    }

    const timer = setTimeout(async () => {
      try {
        const result = await invoke<ParsedDriveLink>("parse_link", { url: driveUrl });
        if (sizeCalcGenRef.current !== currentGen) return;
        setParsedLink(result);
        if (result.is_valid && !projectName) {
          setProjectName(`Project_${result.id.slice(0, 8)}`);
        }

        if (result.is_valid && engineStatus?.has_gdrive) {
          setIsCalculatingSize(true);
          try {
            const sizeRes = await invoke<{ count: number; bytes: number; gb: number }>("get_directory_size", {
              path: result.connection_string,
            });
            if (sizeCalcGenRef.current === currentGen && sizeRes && typeof sizeRes.bytes === "number") {
              setEstimatedSize({ count: sizeRes.count, bytes: sizeRes.bytes });
            }
          } catch (_) {
            // Non-blocking if size calculation is unavailable
          } finally {
            if (sizeCalcGenRef.current === currentGen) {
              setIsCalculatingSize(false);
            }
          }
        }
      } catch (err) {
        console.error("Failed to parse link:", err);
      }
    }, 300);

    return () => clearTimeout(timer);
  }, [driveUrl, engineStatus]);

  // Check destination drive storage whenever path changes
  const checkStorage = async (path: string) => {
    if (!path) return;
    setErrorMsg(null);
    try {
      const info = await invoke<StorageInfo>("probe_storage_write", { path });
      setStorageInfo(info);
    } catch (err) {
      console.error("Storage check failed:", err);
    }
  };

  const handleSelectDestination = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Destination SSD or Folder",
      });
      if (selected && typeof selected === "string") {
        setDestinationPath(selected);
        checkStorage(selected);
      }
    } catch (err) {
      console.error("Folder picker error:", err);
    }
  };

  const handleStartDownload = (bypassDiskSpace = false) => {
    setErrorMsg(null);

    if (!engineStatus?.has_gdrive) {
      setErrorMsg("Google Drive is not connected yet. Please click 'Connect Google Account' below to sign in.");
      return;
    }

    if (!parsedLink || !parsedLink.is_valid) {
      setErrorMsg("Please enter a valid Google Drive folder or file link.");
      return;
    }

    if (!destinationPath) {
      setErrorMsg("Please choose a destination folder or external SSD.");
      return;
    }

    if (storageInfo && !storageInfo.is_writable) {
      setErrorMsg("Destination drive is not writable. Check if the drive is locked or read-only (e.g. NTFS on Mac).");
      return;
    }

    // Capacity Pre-flight Check with Download Anyway override option
    if (storageInfo && estimatedSize && estimatedSize.bytes > storageInfo.free_bytes && !bypassDiskSpace) {
      setDiskSpaceWarning(
        `Project requires ${(estimatedSize.bytes / (1024 * 1024 * 1024)).toFixed(2)} GB, but destination drive only has ${(storageInfo.free_bytes / (1024 * 1024 * 1024)).toFixed(2)} GB free.`
      );
      return;
    }

    setDiskSpaceWarning(null);

    // Determine final target path
    const safeProjectName = projectName.trim().replace(/[/\\?%*:|"<>]/g, "_") || "Project_Download";
    const finalDst = encloseFolder
      ? `${destinationPath.replace(/\/+$/, "")}/${safeProjectName}`
      : destinationPath;

    onStartTransfer(
      parsedLink.connection_string,
      finalDst,
      safeProjectName,
      parsedLink.is_file
    );
  };

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      {/* Intro header card */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 shadow-sm">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-lg font-bold text-slate-900 dark:text-white flex items-center gap-2">
              <Download className="w-5 h-5 text-blue-600 dark:text-blue-400" />
              Download Media Project
            </h1>
            <p className="text-xs text-slate-500 dark:text-zinc-400 mt-1">
              Download complete folders directly to your external SSD with resume support and zero ZIP splitting.
            </p>
          </div>
          <div className="hidden sm:flex items-center gap-1.5 px-3 py-1 rounded-lg bg-blue-50 dark:bg-blue-950/60 border border-blue-200/80 dark:border-blue-800/40 text-xs text-blue-700 dark:text-blue-300 font-medium">
            <Zap className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
            128 MB Chunk Streams
          </div>
        </div>
      </div>

      {/* Step 1: Google Drive Link Input */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-4 shadow-sm">
        <div className="flex items-center justify-between">
          <label className="text-xs font-bold uppercase tracking-wider text-slate-700 dark:text-zinc-300 flex items-center gap-2">
            <Link2 className="w-4 h-4 text-blue-600 dark:text-blue-400" />
            1. Paste Google Drive Link
          </label>
          <span className="text-[11px] text-slate-400 dark:text-zinc-500">Folders, Shared Links, or File IDs</span>
        </div>

        <div>
          <input
            type="text"
            value={driveUrl}
            onChange={(e) => setDriveUrl(e.target.value)}
            placeholder="https://drive.google.com/drive/folders/1A2B3C4D5E6F... or shared link"
            className="w-full bg-slate-50 dark:bg-zinc-950 border border-slate-200 dark:border-zinc-800 rounded-xl px-4 py-3 text-xs text-slate-900 dark:text-white placeholder-slate-400 dark:placeholder-zinc-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20 focus:border-blue-500 font-mono transition"
          />
        </div>

        {/* Link Parsing Status Feedback */}
        {parsedLink && (
          <div className="text-xs transition-all">
            {parsedLink.is_valid ? (
              <div className="flex items-center flex-wrap gap-2 text-emerald-800 dark:text-emerald-300 bg-emerald-50 dark:bg-emerald-950/40 border border-emerald-200 dark:border-emerald-800/50 px-3.5 py-2.5 rounded-xl">
                <CheckCircle2 className="w-4 h-4 text-emerald-600 dark:text-emerald-400 flex-shrink-0" />
                <span className="font-semibold">Valid Google Drive {parsedLink.is_folder ? "Folder" : "File"}:</span>
                <span className="font-mono bg-white dark:bg-zinc-900 px-2 py-0.5 rounded text-slate-800 dark:text-zinc-200 border border-emerald-200 dark:border-zinc-800">
                  ID: {parsedLink.id}
                </span>
                {parsedLink.resource_key && (
                  <span className="bg-blue-100 dark:bg-blue-900/60 text-blue-800 dark:text-blue-300 px-2 py-0.5 rounded border border-blue-200 dark:border-blue-700/60 font-medium">
                    ResourceKey Attached
                  </span>
                )}
                {isCalculatingSize && (
                  <span className="text-[11px] text-slate-500 dark:text-zinc-400 animate-pulse ml-auto">
                    Calculating size...
                  </span>
                )}
                {!isCalculatingSize && estimatedSize && (
                  <span className="bg-white dark:bg-zinc-900 px-2 py-0.5 rounded text-[11px] font-mono text-slate-700 dark:text-zinc-300 border border-emerald-200 dark:border-zinc-800 ml-auto">
                    {(estimatedSize.bytes / (1024 * 1024 * 1024)).toFixed(2)} GB ({estimatedSize.count} files)
                  </span>
                )}
              </div>
            ) : (
              <div className="flex items-center gap-2 text-rose-700 dark:text-rose-400 bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-800/50 px-3.5 py-2.5 rounded-xl">
                <AlertTriangle className="w-4 h-4 flex-shrink-0" />
                <span>{parsedLink.error || "Please enter a valid Google Drive folder or file link."}</span>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Step 2: Destination SSD & Project Folder */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-4 shadow-sm">
        <label className="text-xs font-bold uppercase tracking-wider text-slate-700 dark:text-zinc-300 flex items-center gap-2">
          <FolderDown className="w-4 h-4 text-blue-600 dark:text-blue-400" />
          2. Destination Local SSD / Folder
        </label>

        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div className="sm:col-span-2">
            <label className="text-xs text-slate-600 dark:text-zinc-400 block mb-1 font-medium">Destination Directory</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={destinationPath}
                onChange={(e) => {
                  setDestinationPath(e.target.value);
                  checkStorage(e.target.value);
                }}
                placeholder="/Volumes/Extreme_SSD or C:\Projects"
                className="flex-1 bg-slate-50 dark:bg-zinc-950 border border-slate-200 dark:border-zinc-800 rounded-xl px-3.5 py-2.5 text-xs text-slate-800 dark:text-zinc-200 font-mono focus:outline-none focus:border-blue-500"
              />
              <button
                onClick={handleSelectDestination}
                className="px-4 py-2.5 bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-700 dark:text-zinc-200 rounded-xl text-xs font-semibold border border-slate-200 dark:border-zinc-700 transition flex items-center gap-1.5"
              >
                <HardDrive className="w-3.5 h-3.5" />
                Browse SSD
              </button>
            </div>
          </div>

          <div>
            <label className="text-xs text-slate-600 dark:text-zinc-400 block mb-1 font-medium">Project Folder Name</label>
            <input
              type="text"
              value={projectName}
              onChange={(e) => setProjectName(e.target.value)}
              placeholder="Commercial_Shoot_Day1"
              className="w-full bg-slate-50 dark:bg-zinc-950 border border-slate-200 dark:border-zinc-800 rounded-xl px-3.5 py-2.5 text-xs text-slate-800 dark:text-zinc-200 focus:outline-none focus:border-blue-500"
            />
          </div>
        </div>

        {/* Enclose in subfolder safety checkbox */}
        <div className="flex items-center gap-2 pt-1">
          <input
            type="checkbox"
            id="enclose"
            checked={encloseFolder}
            onChange={(e) => setEncloseFolder(e.target.checked)}
            className="rounded border-slate-300 dark:border-zinc-700 text-blue-600 focus:ring-blue-500 w-4 h-4 cursor-pointer"
          />
          <label htmlFor="enclose" className="text-xs text-slate-600 dark:text-zinc-400 cursor-pointer font-medium">
            Enclose in dedicated subfolder:{" "}
            <span className="font-mono text-slate-900 dark:text-zinc-200">
              {destinationPath.replace(/\/+$/, "")}/{projectName || "Project"}
            </span>{" "}
            (Prevents dumping loose files onto root drive)
          </label>
        </div>

        {/* Hardware & Filesystem Storage Card */}
        {storageInfo && (
          <div className="bg-slate-50 dark:bg-zinc-950/70 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 space-y-2 mt-2">
            <div className="flex items-center justify-between text-xs">
              <span className="text-slate-600 dark:text-zinc-400 flex items-center gap-1.5">
                <HardDrive className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
                Available Storage on {storageInfo.mount_point || "Selected Drive"}:
              </span>
              <span className="font-semibold text-slate-800 dark:text-zinc-200">
                {storageInfo.free_gb.toFixed(1)} GB free / {storageInfo.total_gb.toFixed(1)} GB ({storageInfo.file_system})
              </span>
            </div>

            {/* Storage bar */}
            <div className="w-full h-2 rounded-full bg-slate-200 dark:bg-zinc-800 overflow-hidden">
              <div
                className="h-full bg-blue-600 dark:bg-blue-500 rounded-full"
                style={{
                  width: `${
                    storageInfo.total_gb > 0
                      ? Math.min(100, (1 - storageInfo.free_gb / storageInfo.total_gb) * 100)
                      : 0
                  }%`,
                }}
              />
            </div>

            {/* FAT32 Warning */}
            {storageInfo.is_fat32 && (
              <div className="flex items-center gap-2 text-xs text-amber-800 dark:text-amber-300 bg-amber-50 dark:bg-amber-950/40 border border-amber-200 dark:border-amber-800/50 p-2.5 rounded-lg mt-2">
                <AlertTriangle className="w-4 h-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
                <span>
                  <strong>Warning:</strong> Drive is formatted as FAT32. Individual video files larger than 4GB will fail to write. Recommended format: exFAT or APFS.
                </span>
              </div>
            )}

            {/* NTFS Read Only Error on Mac */}
            {!storageInfo.is_writable && (
              <div className="flex items-center gap-2 text-xs text-rose-800 dark:text-rose-300 bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-800/50 p-2.5 rounded-lg mt-2">
                <AlertTriangle className="w-4 h-4 flex-shrink-0 text-rose-600 dark:text-rose-400" />
                <span>
                  <strong>Error:</strong> Cannot write to this folder. It may be a Windows NTFS drive (Read-Only on Mac) or requires write permissions.
                </span>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Error alert if any */}
      {errorMsg && (
        <div className="p-4 bg-rose-50 dark:bg-rose-950/60 border border-rose-200 dark:border-rose-800 rounded-xl text-rose-800 dark:text-rose-200 text-xs flex items-center gap-2">
          <AlertTriangle className="w-4 h-4 flex-shrink-0 text-rose-600 dark:text-rose-400" />
          <span>{errorMsg}</span>
        </div>
      )}

      {/* Google Account Not Connected Notice */}
      {!engineStatus?.has_gdrive && (
        <div className="p-4 bg-amber-50 dark:bg-amber-950/60 border border-amber-200 dark:border-amber-800 rounded-xl text-xs space-y-2.5">
          <div className="flex items-center gap-2 font-semibold text-amber-900 dark:text-amber-300">
            <AlertTriangle className="w-4 h-4 text-amber-600 dark:text-amber-400 flex-shrink-0" />
            <span>Google Drive account is not connected</span>
          </div>
          <p className="text-amber-800 dark:text-amber-400/90 text-[11px] leading-relaxed">
            Google requires account authentication to download shared folders. Please connect your Google account once (takes 5 seconds).
          </p>
          <button
            onClick={onGoToSettings}
            className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-semibold text-xs transition shadow-sm"
          >
            Connect Google Account
          </button>
        </div>
      )}

      {/* Low Disk Space Warning with Override Option */}
      {diskSpaceWarning && (
        <div className="p-4 bg-amber-50 dark:bg-amber-950/60 border border-amber-300 dark:border-amber-750 rounded-xl text-xs space-y-3 shadow-sm animate-in fade-in duration-200">
          <div className="flex items-start gap-2.5">
            <AlertTriangle className="w-5 h-5 text-amber-600 dark:text-amber-400 flex-shrink-0 mt-0.5" />
            <div>
              <span className="font-bold text-amber-950 dark:text-amber-200 block text-xs">
                Insufficient Disk Space Warning
              </span>
              <p className="text-amber-900 dark:text-amber-300/90 text-[11px] leading-relaxed mt-0.5">
                {diskSpaceWarning}
              </p>
              <p className="text-[11px] text-amber-800/90 dark:text-amber-400/90 mt-1">
                You can still continue downloading now. Files will stream until the drive fills up or as you free up disk space in real-time.
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2 pt-1">
            <button
              type="button"
              onClick={() => handleStartDownload(true)}
              className="px-4 py-2 bg-amber-600 hover:bg-amber-700 text-white rounded-lg font-bold text-xs transition shadow-sm flex items-center gap-1.5"
            >
              <Download className="w-3.5 h-3.5" />
              Download Anyway
            </button>
            <button
              type="button"
              onClick={() => setDiskSpaceWarning(null)}
              className="px-3 py-2 bg-slate-200 hover:bg-slate-300 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-700 dark:text-zinc-300 rounded-lg text-xs font-medium transition"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Action Button */}
      <div className="pt-2">
        <button
          onClick={() => handleStartDownload(false)}
          disabled={!parsedLink?.is_valid || Boolean(storageInfo && !storageInfo.is_writable) || isCalculatingSize}
          className="w-full py-3.5 rounded-xl font-bold text-sm bg-blue-600 hover:bg-blue-700 dark:bg-blue-600 dark:hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed text-white shadow-md shadow-blue-500/20 transition flex items-center justify-center gap-2"
        >
          {isCalculatingSize ? (
            <RotateCw className="w-4 h-4 animate-spin" />
          ) : (
            <Download className="w-4 h-4" />
          )}
          {isCalculatingSize ? "CALCULATING PROJECT SIZE..." : "START DOWNLOAD PROJECT"}
        </button>
        <p className="text-center text-[11px] text-slate-500 dark:text-zinc-500 mt-2 flex items-center justify-center gap-1.5">
          <ShieldCheck className="w-3.5 h-3.5 text-emerald-500" />
          Zero ZIP splitting • In-flight resume support • Bit-for-bit MD5 verification
        </p>
      </div>
    </div>
  );
};
