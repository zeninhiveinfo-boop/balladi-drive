import React from "react";
import { CheckCircle2, AlertTriangle, Sun, Moon } from "lucide-react";
import { AppEngineStatus } from "../types";
import logoLight from "../assets/logo-light.png";
import logoDark from "../assets/logo-dark.png";

interface HeaderProps {
  activeTab: "download" | "upload" | "transfers" | "history" | "settings";
  setActiveTab: (tab: "download" | "upload" | "transfers" | "history" | "settings") => void;
  engineStatus: AppEngineStatus | null;
  activeTransferCount: number;
  isDark: boolean;
  setIsDark: (dark: boolean) => void;
}

export const Header: React.FC<HeaderProps> = ({
  activeTab,
  setActiveTab,
  engineStatus,
  activeTransferCount,
  isDark,
  setIsDark,
}) => {
  return (
    <header className="border-b border-slate-200 dark:border-zinc-800 bg-white/90 dark:bg-zinc-950/90 backdrop-blur sticky top-0 z-50 transition-colors">
      <div className="max-w-7xl mx-auto px-6 h-[70px] flex items-center justify-between">
        {/* Brand */}
        <div className="flex items-center gap-3.5">
          <img
            src={isDark ? logoDark : logoLight}
            alt="Balladi Studios"
            className="h-12 w-auto object-contain transition-all duration-200"
          />
          <div>
            <span className="font-bold tracking-tight text-slate-900 dark:text-white text-base leading-tight block">
              Balladi Drive
            </span>
            <p className="text-[11px] text-slate-500 dark:text-zinc-400 font-medium leading-none mt-0.5">
              Media Transfer Engine
            </p>
          </div>
        </div>

        {/* Navigation Segmented Control */}
        <nav className="flex items-center bg-slate-100 dark:bg-zinc-900 p-1 rounded-xl border border-slate-200/80 dark:border-zinc-800">
          <button
            onClick={() => setActiveTab("download")}
            className={`px-3.5 py-1.5 rounded-lg text-xs font-semibold transition-all ${
              activeTab === "download"
                ? "bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-sm"
                : "text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-zinc-200"
            }`}
          >
            Download
          </button>
          <button
            onClick={() => setActiveTab("upload")}
            className={`px-3.5 py-1.5 rounded-lg text-xs font-semibold transition-all ${
              activeTab === "upload"
                ? "bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-sm"
                : "text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-zinc-200"
            }`}
          >
            Upload
          </button>
          <button
            onClick={() => setActiveTab("transfers")}
            className={`px-3.5 py-1.5 rounded-lg text-xs font-semibold transition-all relative ${
              activeTab === "transfers"
                ? "bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-sm"
                : "text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-zinc-200"
            }`}
          >
            Transfers
            {activeTransferCount > 0 && (
              <span className="ml-1.5 px-1.5 py-0.2 text-[10px] rounded-full bg-blue-600 text-white font-bold animate-pulse">
                {activeTransferCount}
              </span>
            )}
          </button>
          <button
            onClick={() => setActiveTab("history")}
            className={`px-3.5 py-1.5 rounded-lg text-xs font-semibold transition-all ${
              activeTab === "history"
                ? "bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-sm"
                : "text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-zinc-200"
            }`}
          >
            History & Verify
          </button>
          <button
            onClick={() => setActiveTab("settings")}
            className={`px-3.5 py-1.5 rounded-lg text-xs font-semibold transition-all ${
              activeTab === "settings"
                ? "bg-white dark:bg-zinc-800 text-slate-900 dark:text-white shadow-sm"
                : "text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-zinc-200"
            }`}
          >
            Settings
          </button>
        </nav>

        {/* Right Tools: Status & Theme Switcher */}
        <div className="flex items-center gap-2.5">
          {/* Theme Switcher Button */}
          <button
            onClick={() => setIsDark(!isDark)}
            title={isDark ? "Switch to Light Theme" : "Switch to Dark Theme"}
            className="p-2 rounded-xl bg-slate-100 dark:bg-zinc-900 hover:bg-slate-200 dark:hover:bg-zinc-800 border border-slate-200 dark:border-zinc-800 text-slate-600 dark:text-zinc-300 transition"
          >
            {isDark ? <Sun className="w-4 h-4 text-amber-400" /> : <Moon className="w-4 h-4 text-slate-600" />}
          </button>

          {/* Drive status badge & active account */}
          <div
            onClick={() => setActiveTab("settings")}
            title={
              engineStatus?.user_info?.display_name
                ? `${engineStatus.user_info.display_name} (${engineStatus.user_info.email || ""})`
                : engineStatus?.user_info?.email || "Google Drive connection status"
            }
            className="cursor-pointer flex items-center gap-2 px-3 py-1.5 rounded-xl bg-slate-100 dark:bg-zinc-900 hover:bg-slate-200 dark:hover:bg-zinc-800 border border-slate-200 dark:border-zinc-800 text-xs transition"
          >
            {engineStatus?.has_gdrive ? (
              <>
                {engineStatus.user_info?.photo_link ? (
                  <img
                    src={engineStatus.user_info.photo_link}
                    alt="User"
                    className="w-4 h-4 rounded-full object-cover"
                    referrerPolicy="no-referrer"
                  />
                ) : (
                  <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" />
                )}
                <span className="font-semibold text-emerald-700 dark:text-emerald-400 max-w-[160px] truncate">
                  {engineStatus.user_info?.display_name || engineStatus.user_info?.email || "Drive Connected"}
                </span>
              </>
            ) : (
              <>
                <AlertTriangle className="w-3.5 h-3.5 text-amber-500" />
                <span className="font-semibold text-amber-700 dark:text-amber-400">Setup Drive</span>
              </>
            )}
          </div>
        </div>
      </div>
    </header>
  );
};
