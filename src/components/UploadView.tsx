import React, { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Upload,
  FolderUp,
  Cloud,
  CheckCircle2,
  AlertTriangle,
  FolderGit2,
  FileUp,
  ShieldCheck,
  Zap,
} from "lucide-react";

interface UploadViewProps {
  onStartTransfer: (
    source: string,
    destination: string,
    projectName: string,
    isSingleFile?: boolean
  ) => void;
}

export const UploadView: React.FC<UploadViewProps> = ({ onStartTransfer }) => {
  const [sourcePath, setSourcePath] = useState("");
  const [projectName, setProjectName] = useState("");
  const [cloudDestination, setCloudDestination] = useState("gdrive:/Media_Projects");
  const [isSingleFile, setIsSingleFile] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const handleSelectFolder = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Local Media Project or Camera Card",
      });
      if (selected && typeof selected === "string") {
        setSourcePath(selected);
        setIsSingleFile(false);
        const inferredName = selected.split(/[/\\]/).filter(Boolean).pop() || "Project";
        setProjectName(inferredName);
      }
    } catch (err) {
      console.error("Folder picker error:", err);
    }
  };

  const handleSelectFile = async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        title: "Select Local File to Upload",
      });
      if (selected && typeof selected === "string") {
        setSourcePath(selected);
        setIsSingleFile(true);
        const inferredName = selected.split(/[/\\]/).filter(Boolean).pop() || "File_Upload";
        setProjectName(inferredName);
      }
    } catch (err) {
      console.error("File picker error:", err);
    }
  };

  const handleStartUpload = () => {
    setErrorMsg(null);
    if (!sourcePath) {
      setErrorMsg("Please select a local project folder, camera card, or file.");
      return;
    }

    const safeName =
      projectName.trim().replace(/[/\\?%*:|"<>]/g, "_") ||
      "Project_Upload";

    const baseCloudDst = cloudDestination.replace(/\/+$/, "");

    const finalCloudDst = isSingleFile
      ? baseCloudDst
      : `${baseCloudDst}/${safeName}`;

    onStartTransfer(sourcePath, finalCloudDst, safeName, isSingleFile);
  };

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      {/* Intro Header */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 shadow-sm">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-lg font-bold text-slate-900 dark:text-white flex items-center gap-2">
              <Upload className="w-5 h-5 text-blue-600 dark:text-blue-400" />
              Upload Project to Google Drive
            </h1>
            <p className="text-xs text-slate-500 dark:text-zinc-400 mt-1">
              Upload multi-hundred-gigabyte shoots directly to Google Drive with 128MB chunks and camera card hierarchy preservation.
            </p>
          </div>
          <div className="hidden sm:flex items-center gap-1.5 px-3 py-1 rounded-lg bg-blue-50 dark:bg-blue-950/60 border border-blue-200/80 dark:border-blue-800/40 text-xs text-blue-700 dark:text-blue-300 font-medium">
            <Zap className="w-3.5 h-3.5 text-blue-600 dark:text-blue-400" />
            Camera Card Safe
          </div>
        </div>
      </div>

      {/* Step 1: Local Source Folder / File */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-4 shadow-sm">
        <label className="text-xs font-bold uppercase tracking-wider text-slate-700 dark:text-zinc-300 flex items-center gap-2">
          <FolderUp className="w-4 h-4 text-blue-600 dark:text-blue-400" />
          1. Choose Local Project Folder, Camera Card, or File
        </label>

        <div className="flex flex-col sm:flex-row gap-2.5">
          <input
            type="text"
            value={sourcePath}
            readOnly
            placeholder="Select a local folder or file using the buttons"
            className="flex-1 bg-slate-100 dark:bg-zinc-950/80 border border-slate-200 dark:border-zinc-800 rounded-xl px-4 py-3 text-xs text-slate-800 dark:text-zinc-200 font-mono focus:outline-none cursor-default"
          />
          <div className="flex items-center gap-2">
            <button
              onClick={handleSelectFolder}
              className="flex-1 sm:flex-initial px-4 py-3 bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-800 dark:text-zinc-100 rounded-xl text-xs font-bold border border-slate-200 dark:border-zinc-700 transition flex items-center justify-center gap-1.5"
              title="Browse local project folder or camera card"
            >
              <FolderGit2 className="w-4 h-4 text-blue-600 dark:text-blue-400" />
              Folder
            </button>
            <button
              onClick={handleSelectFile}
              className="flex-1 sm:flex-initial px-4 py-3 bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-800 dark:text-zinc-100 rounded-xl text-xs font-bold border border-slate-200 dark:border-zinc-700 transition flex items-center justify-center gap-1.5"
              title="Browse single video/image file to upload"
            >
              <FileUp className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
              File
            </button>
          </div>
        </div>

        {sourcePath && (
          <div className="bg-slate-50 dark:bg-zinc-950/60 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 flex items-center justify-between text-xs">
            <div className="flex items-center gap-2 text-slate-700 dark:text-zinc-300">
              <CheckCircle2 className="w-4 h-4 text-emerald-500" />
              <span>Target detected:</span>
              <span className="font-semibold text-slate-900 dark:text-white">{projectName}</span>
            </div>
            <span className="text-slate-400 dark:text-zinc-500 font-mono text-[11px] truncate max-w-xs">{sourcePath}</span>
          </div>
        )}
      </div>

      {/* Step 2: Google Drive Destination */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-4 shadow-sm">
        <label className="text-xs font-bold uppercase tracking-wider text-slate-700 dark:text-zinc-300 flex items-center gap-2">
          <Cloud className="w-4 h-4 text-blue-600 dark:text-blue-400" />
          2. Google Drive Destination Folder
        </label>

        <div>
          <label className="text-xs text-slate-600 dark:text-zinc-400 block mb-1 font-medium">Cloud Target Path</label>
          <input
            type="text"
            value={cloudDestination}
            onChange={(e) => setCloudDestination(e.target.value)}
            placeholder="gdrive:/Media_Projects or gdrive:/Raw_Footage"
            className="w-full bg-slate-50 dark:bg-zinc-950 border border-slate-200 dark:border-zinc-800 rounded-xl px-4 py-3 text-xs text-slate-900 dark:text-zinc-200 font-mono focus:outline-none focus:border-blue-500"
          />
          <p className="text-[11px] text-slate-400 dark:text-zinc-500 mt-2 font-medium">
            Files will be uploaded directly to:{" "}
            <span className="font-mono text-slate-800 dark:text-zinc-300">
              {cloudDestination.replace(/\/+$/, "")}/{projectName || "Project"}
            </span>
          </p>
        </div>

        {/* Safety Note on Camera Cards */}
        <div className="bg-slate-50 dark:bg-zinc-950/70 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 text-xs text-slate-600 dark:text-zinc-400 space-y-1">
          <div className="flex items-center gap-2 text-slate-900 dark:text-zinc-200 font-semibold">
            <ShieldCheck className="w-4 h-4 text-emerald-500" />
            Media Safety & Metadata Guarantee
          </div>
          <p className="text-[11px] text-slate-500 dark:text-zinc-400 leading-relaxed">
            • Empty directories (`RDC/`, `THUMB/`) and 0-byte `.XML` / `.BUP` files are strictly retained.<br />
            • Mac `.DS_Store` and hidden OS files are automatically cleaned up to prevent Windows NLE conflicts.<br />
            • Resumable chunking guarantees that interrupted Wi-Fi does not restart uploads from zero.
          </p>
        </div>
      </div>

      {/* Error alert if any */}
      {errorMsg && (
        <div className="p-4 bg-rose-50 dark:bg-rose-950/60 border border-rose-200 dark:border-rose-800 rounded-xl text-rose-800 dark:text-rose-200 text-xs flex items-center gap-2">
          <AlertTriangle className="w-4 h-4 flex-shrink-0 text-rose-600 dark:text-rose-400" />
          <span>{errorMsg}</span>
        </div>
      )}

      {/* Action Button */}
      <div className="pt-2">
        <button
          onClick={handleStartUpload}
          disabled={!sourcePath}
          className="w-full py-3.5 rounded-xl font-bold text-sm bg-blue-600 hover:bg-blue-700 dark:bg-blue-600 dark:hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed text-white shadow-md shadow-blue-500/20 transition flex items-center justify-center gap-2"
        >
          <Upload className="w-4 h-4" />
          UPLOAD PROJECT TO GOOGLE DRIVE
        </button>
      </div>
    </div>
  );
};
