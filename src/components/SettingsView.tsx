import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Settings,
  Cloud,
  Bell,
  CheckCircle2,
  ExternalLink,
  RotateCw,
  LogOut,
  AlertTriangle,
  ChevronDown,
  ChevronUp,
  ShieldCheck,
} from "lucide-react";
import { AppEngineStatus, PublicAppSettings } from "../types";

interface SettingsViewProps {
  engineStatus: AppEngineStatus | null;
  onRefreshEngineStatus: () => void;
}

export const SettingsView: React.FC<SettingsViewProps> = ({
  engineStatus,
  onRefreshEngineStatus,
}) => {
  const [oauthMode, setOauthMode] = useState<"managed" | "custom">("managed");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [customClientId, setCustomClientId] = useState("");
  const [hasCustomClientId, setHasCustomClientId] = useState(false);
  const [clearCustomClientId, setClearCustomClientId] = useState(false);
  const [customClientSecret, setCustomClientSecret] = useState("");
  const [hasCustomSecret, setHasCustomSecret] = useState(false);
  const [clearCustomSecret, setClearCustomSecret] = useState(false);
  
  const [webhookUrl, setWebhookUrl] = useState("");
  const [hasWebhookUrl, setHasWebhookUrl] = useState(false);
  const [clearWebhookUrl, setClearWebhookUrl] = useState(false);
  const [notifyOnComplete, setNotifyOnComplete] = useState(true);
  
  const [isConnecting, setIsConnecting] = useState(false);
  const [authFeedback, setAuthFeedback] = useState<{ type: "success" | "error" | "info"; msg: string } | null>(null);
  const [savedSuccess, setSavedSuccess] = useState(false);
  const [userInfo, setUserInfo] = useState(engineStatus?.user_info || null);

  // Load secure settings on mount
  useEffect(() => {
    invoke<PublicAppSettings>("get_app_settings")
      .then((settings) => {
        if (settings) {
          setOauthMode(settings.oauth_mode || "managed");
          setHasCustomClientId(Boolean(settings.has_custom_client_id));
          setHasCustomSecret(Boolean(settings.has_custom_client_secret));
          setHasWebhookUrl(Boolean(settings.has_webhook_url));
          setNotifyOnComplete(settings.notify_on_complete !== false);
        }
      })
      .catch((e) => console.error("Could not load app settings:", e));
  }, []);

  // Sync engineStatus user_info or fetch fresh
  useEffect(() => {
    if (engineStatus?.user_info?.email) {
      setUserInfo(engineStatus.user_info);
    } else {
      invoke<any>("get_google_user_info")
        .then((info) => {
          if (info && info.email) {
            setUserInfo(info);
          }
        })
        .catch((e) => console.error("Could not fetch user profile:", e));
    }
  }, [engineStatus]);

  const handleConnectGoogle = async () => {
    setIsConnecting(true);
    setAuthFeedback({
      type: "info",
      msg: "Opening your browser... Please sign in with your Google account and click 'Allow' on the consent screen.",
    });

    try {
      const candidateId = customClientId.trim()
        ? customClientId.trim()
        : hasCustomClientId && !clearCustomClientId
        ? "__PRESERVE__"
        : null;

      const candidateSecret = customClientSecret.trim()
        ? customClientSecret.trim()
        : hasCustomSecret && !clearCustomSecret
        ? "__PRESERVE__"
        : null;

      const res: any = await invoke("connect_google_drive", {
        oauthMode,
        customClientId: oauthMode === "custom" ? candidateId : null,
        customClientSecret: oauthMode === "custom" ? candidateSecret : null,
      });

      if (res && res.success) {
        setAuthFeedback({
          type: "success",
          msg: "Google Account connected successfully! You can now download and upload projects.",
        });
        try {
          const freshUser = await invoke<any>("get_google_user_info");
          if (freshUser) setUserInfo(freshUser);
        } catch (_) {}
        onRefreshEngineStatus();
      } else {
        setAuthFeedback({
          type: "error",
          msg: res?.error || "Authentication was not completed.",
        });
      }
    } catch (err: any) {
      console.error("Auth error:", err);
      setAuthFeedback({
        type: "error",
        msg: `Connection failed: ${err?.message || err}`,
      });
    } finally {
      setIsConnecting(false);
    }
  };

  const handleDisconnect = async () => {
    try {
      await invoke("disconnect_google_drive");
      setAuthFeedback({
        type: "info",
        msg: "Google Account disconnected.",
      });
      onRefreshEngineStatus();
    } catch (err) {
      console.error("Disconnect error:", err);
    }
  };

  const handleSaveSettings = async () => {
    try {
      const savedWebhookUrl = clearWebhookUrl
        ? ""
        : webhookUrl.trim()
        ? webhookUrl.trim()
        : hasWebhookUrl
        ? "__PRESERVE__"
        : "";

      const savedCustomId = clearCustomClientId
        ? ""
        : customClientId.trim()
        ? customClientId.trim()
        : hasCustomClientId
        ? "__PRESERVE__"
        : "";

      const savedCustomSecret = clearCustomSecret
        ? ""
        : customClientSecret.trim()
        ? customClientSecret.trim()
        : hasCustomSecret
        ? "__PRESERVE__"
        : "";

      await invoke("save_app_settings", {
        settings: {
          oauth_mode: oauthMode,
          custom_client_id: oauthMode === "custom" ? savedCustomId : null,
          custom_client_secret: oauthMode === "custom" ? savedCustomSecret : null,
          webhook_url: savedWebhookUrl,
          notify_on_complete: notifyOnComplete,
        },
      });

      setHasWebhookUrl(
        clearWebhookUrl ? false : hasWebhookUrl || Boolean(webhookUrl.trim())
      );
      setHasCustomClientId(
        clearCustomClientId ? false : hasCustomClientId || Boolean(customClientId.trim())
      );
      setHasCustomSecret(
        clearCustomSecret ? false : hasCustomSecret || Boolean(customClientSecret.trim())
      );
      setWebhookUrl("");
      setCustomClientId("");
      setCustomClientSecret("");
      setClearWebhookUrl(false);
      setClearCustomClientId(false);
      setClearCustomSecret(false);
      setSavedSuccess(true);
      setTimeout(() => setSavedSuccess(false), 3000);
    } catch (err) {
      console.error("Failed saving settings:", err);
    }
  };

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-lg font-bold text-slate-900 dark:text-white flex items-center gap-2">
          <Settings className="w-5 h-5 text-blue-600 dark:text-blue-400" />
          Settings & Engine Configuration
        </h2>
        <p className="text-xs text-slate-500 dark:text-zinc-400 mt-1">
          Manage your Google Account connection, automated transfer webhooks, and preferences.
        </p>
      </div>

      {/* Card 1: Balladi Studios Managed Google Connection */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-5 shadow-sm">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/80 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400">
              <Cloud className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-xs font-bold uppercase tracking-wider text-slate-900 dark:text-white">
                Balladi Studios Managed Google Connection
              </h3>
              <p className="text-xs text-slate-500 dark:text-zinc-400">
                Official high-speed Google Drive transfer integration
              </p>
            </div>
          </div>

          <span
            className={`px-3 py-1 rounded-full text-xs font-semibold flex items-center gap-1.5 ${
              engineStatus?.has_gdrive
                ? "bg-emerald-50 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800/60"
                : "bg-amber-50 dark:bg-amber-950/60 text-amber-700 dark:text-amber-400 border border-amber-200 dark:border-amber-800/60"
            }`}
          >
            <CheckCircle2 className="w-3.5 h-3.5" />
            {engineStatus?.has_gdrive ? "Connected" : "Disconnected"}
          </span>
        </div>

        {/* Managed Architecture Explanation */}
        <div className="bg-blue-50/70 dark:bg-blue-950/40 border border-blue-200/80 dark:border-blue-900/60 rounded-xl p-4 text-xs text-slate-700 dark:text-zinc-300 space-y-2">
          <div className="flex items-center gap-2 font-semibold text-blue-900 dark:text-blue-300">
            <ShieldCheck className="w-4 h-4 text-blue-600 dark:text-blue-400 flex-shrink-0" />
            Production Managed OAuth
          </div>
          <ul className="space-y-1.5 text-[11px] text-slate-600 dark:text-zinc-400 list-disc list-inside leading-relaxed">
            <li><strong>Personal Authorization:</strong> You authorize your own personal Google account in the browser.</li>
            <li><strong>Separate Token:</strong> Your authentication token is stored locally on your device only.</li>
            <li><strong>Uploads & Downloads:</strong> Uploads go directly to your connected account; downloads work for files accessible to your account.</li>
            <li><strong>Dedicated Quota:</strong> API traffic routes through the Balladi Drive Production project for dedicated speed and rate limits.</li>
          </ul>
        </div>

        {/* Connected Google Account Details */}
        {engineStatus?.has_gdrive && (userInfo?.email || engineStatus?.user_info?.email) && (() => {
          const activeUser = userInfo || engineStatus?.user_info;
          if (!activeUser?.email) return null;
          return (
            <div className="bg-slate-50 dark:bg-zinc-950/80 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 flex items-center justify-between gap-4">
              <div className="flex items-center gap-3.5">
                {activeUser.photo_link ? (
                  <img
                    src={activeUser.photo_link}
                    alt="Google Avatar"
                    className="w-11 h-11 rounded-full border border-slate-200 dark:border-zinc-700 object-cover shadow-sm"
                    referrerPolicy="no-referrer"
                  />
                ) : (
                  <div className="w-11 h-11 rounded-full bg-blue-600 text-white font-bold flex items-center justify-center text-sm shadow-sm">
                    {activeUser.display_name?.slice(0, 1) || "G"}
                  </div>
                )}

                <div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-bold text-slate-900 dark:text-white">
                      {activeUser.display_name || "Google Account"}
                    </span>
                    <span className="px-2 py-0.5 rounded-full text-[10px] font-semibold bg-emerald-100 dark:bg-emerald-900/50 text-emerald-800 dark:text-emerald-300">
                      Active
                    </span>
                  </div>
                  <p className="text-xs font-mono text-slate-600 dark:text-zinc-400 mt-0.5">
                    {activeUser.email}
                  </p>
                  {activeUser.storage_total && activeUser.storage_used && (
                    <p className="text-[11px] text-slate-500 dark:text-zinc-500 mt-1">
                      Drive Storage: {(activeUser.storage_used / (1024 * 1024 * 1024)).toFixed(1)} GB used of{" "}
                      {(activeUser.storage_total / (1024 * 1024 * 1024 * 1024)).toFixed(1)} TB
                    </p>
                  )}
                </div>
              </div>
            </div>
          );
        })()}

        {/* Feedback Banner */}
        {authFeedback && (
          <div
            className={`p-3.5 rounded-xl text-xs flex items-center gap-2.5 ${
              authFeedback.type === "success"
                ? "bg-emerald-50 dark:bg-emerald-950/50 text-emerald-800 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800"
                : authFeedback.type === "error"
                ? "bg-rose-50 dark:bg-rose-950/50 text-rose-800 dark:text-rose-300 border border-rose-200 dark:border-rose-800"
                : "bg-blue-50 dark:bg-blue-950/50 text-blue-800 dark:text-blue-300 border border-blue-200 dark:border-blue-800"
            }`}
          >
            {authFeedback.type === "success" ? (
              <CheckCircle2 className="w-4 h-4 text-emerald-600 dark:text-emerald-400 flex-shrink-0" />
            ) : authFeedback.type === "error" ? (
              <AlertTriangle className="w-4 h-4 text-rose-600 dark:text-rose-400 flex-shrink-0" />
            ) : (
              <RotateCw className="w-4 h-4 text-blue-600 dark:text-blue-400 animate-spin flex-shrink-0" />
            )}
            <span>{authFeedback.msg}</span>
          </div>
        )}

        <div className="pt-1 flex flex-wrap gap-2.5">
          <button
            onClick={handleConnectGoogle}
            disabled={isConnecting}
            className="px-4 py-2.5 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 text-white rounded-xl text-xs font-semibold transition flex items-center gap-2 shadow-sm"
          >
            {isConnecting ? (
              <RotateCw className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <ExternalLink className="w-3.5 h-3.5" />
            )}
            {isConnecting
              ? "Waiting for Browser Authorization..."
              : engineStatus?.has_gdrive
              ? "Reconnect / Switch Google Account"
              : "Connect Google Account"}
          </button>

          {engineStatus?.has_gdrive && (
            <button
              onClick={handleDisconnect}
              className="px-4 py-2.5 bg-slate-100 hover:bg-slate-200 dark:bg-zinc-800 dark:hover:bg-zinc-700 text-slate-700 dark:text-zinc-300 rounded-xl text-xs font-semibold transition flex items-center gap-1.5 border border-slate-200 dark:border-zinc-700"
            >
              <LogOut className="w-3.5 h-3.5 text-slate-500" />
              Disconnect
            </button>
          )}
        </div>

        {/* Collapsible Advanced Section */}
        <div className="pt-2 border-t border-slate-100 dark:border-zinc-800/80">
          <button
            type="button"
            onClick={() => setShowAdvanced(!showAdvanced)}
            className="text-xs text-slate-500 dark:text-zinc-400 hover:text-slate-700 dark:hover:text-zinc-200 font-medium flex items-center gap-1.5"
          >
            {showAdvanced ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
            Advanced: Custom OAuth Project
          </button>

          {showAdvanced && (
            <div className="mt-3 bg-slate-50 dark:bg-zinc-950/60 border border-slate-200 dark:border-zinc-800 rounded-xl p-4 space-y-3 text-xs">
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="customOauth"
                  checked={oauthMode === "custom"}
                  onChange={(e) => setOauthMode(e.target.checked ? "custom" : "managed")}
                  className="rounded border-slate-300 dark:border-zinc-700 text-blue-600 focus:ring-blue-500 w-4 h-4 cursor-pointer"
                />
                <label htmlFor="customOauth" className="font-semibold text-slate-800 dark:text-zinc-200 cursor-pointer">
                  Use custom Google Cloud project credentials instead of Balladi Managed OAuth
                </label>
              </div>

              {oauthMode === "custom" && (
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-2">
                  <div>
                    <div className="flex items-center justify-between mb-1">
                      <label className="text-[11px] text-slate-600 dark:text-zinc-400 font-medium">
                        Custom Client ID
                      </label>
                      {hasCustomClientId && !clearCustomClientId && (
                        <button
                          type="button"
                          onClick={() => {
                            setCustomClientId("");
                            setClearCustomClientId(true);
                          }}
                          className="text-[10px] text-rose-500 hover:text-rose-600 font-medium"
                        >
                          Clear ID
                        </button>
                      )}
                    </div>
                    <input
                      type="text"
                      value={customClientId}
                      onChange={(e) => {
                        setCustomClientId(e.target.value);
                        if (clearCustomClientId) setClearCustomClientId(false);
                      }}
                      placeholder={hasCustomClientId && !clearCustomClientId ? "•••••••••••••••• (Saved)" : "e.g. 123456789.apps.googleusercontent.com"}
                      className="w-full bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-lg px-3 py-2 text-xs font-mono"
                    />
                  </div>
                  <div>
                    <div className="flex items-center justify-between mb-1">
                      <label className="text-[11px] text-slate-600 dark:text-zinc-400 font-medium">
                        Custom Client Secret
                      </label>
                      {hasCustomSecret && !clearCustomSecret && (
                        <button
                          type="button"
                          onClick={() => {
                            setCustomClientSecret("");
                            setClearCustomSecret(true);
                          }}
                          className="text-[10px] text-rose-500 hover:text-rose-600 font-medium"
                        >
                          Clear secret
                        </button>
                      )}
                    </div>
                    <input
                      type="password"
                      value={customClientSecret}
                      onChange={(e) => {
                        setCustomClientSecret(e.target.value);
                        if (clearCustomSecret) setClearCustomSecret(false);
                      }}
                      placeholder={hasCustomSecret && !clearCustomSecret ? "••••••••••••••••" : "GOCSPX-..."}
                      className="w-full bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-lg px-3 py-2 text-xs font-mono"
                    />
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Card 2: Unattended Transfer Webhooks (Slack / Discord) */}
      <div className="bg-white dark:bg-zinc-900 border border-slate-200 dark:border-zinc-800 rounded-2xl p-6 space-y-4 shadow-sm">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl bg-emerald-50 dark:bg-emerald-950/80 border border-emerald-200 dark:border-emerald-800/60 flex items-center justify-center text-emerald-600 dark:text-emerald-400">
            <Bell className="w-5 h-5" />
          </div>
          <div>
            <h3 className="text-xs font-bold uppercase tracking-wider text-slate-900 dark:text-white">Overnight Webhook Alerts</h3>
            <p className="text-xs text-slate-500 dark:text-zinc-400">Receive a ping in Slack or Discord when a transfer completes</p>
          </div>
        </div>

        <div>
          <div className="flex items-center justify-between mb-1">
            <label className="text-xs text-slate-700 dark:text-zinc-300 font-medium">Webhook URL (Slack or Discord)</label>
            {hasWebhookUrl && !clearWebhookUrl && (
              <button
                type="button"
                onClick={() => {
                  setWebhookUrl("");
                  setClearWebhookUrl(true);
                }}
                className="text-[10px] text-rose-500 hover:text-rose-600 font-medium"
              >
                Clear webhook
              </button>
            )}
          </div>
          <input
            type="text"
            value={webhookUrl}
            onChange={(e) => {
              setWebhookUrl(e.target.value);
              if (clearWebhookUrl) setClearWebhookUrl(false);
            }}
            placeholder={hasWebhookUrl && !clearWebhookUrl ? "•••••••••••••••• (Configured and masked)" : "https://hooks.slack.com/services/... or https://discord.com/api/webhooks/..."}
            className="w-full bg-slate-50 dark:bg-zinc-950 border border-slate-200 dark:border-zinc-800 rounded-xl px-3.5 py-2.5 text-xs text-slate-800 dark:text-zinc-200 font-mono focus:outline-none focus:border-blue-500"
          />
        </div>

        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id="notify"
            checked={notifyOnComplete}
            onChange={(e) => setNotifyOnComplete(e.target.checked)}
            className="rounded border-slate-300 dark:border-zinc-700 text-blue-600 focus:ring-blue-500 w-4 h-4 cursor-pointer"
          />
          <label htmlFor="notify" className="text-xs text-slate-600 dark:text-zinc-400 cursor-pointer font-medium">
            Post automated message with file count, transferred size, and MD5 status upon completion
          </label>
        </div>
      </div>

      {/* Save Settings Bar */}
      <div className="flex items-center justify-between pt-2">
        <div className="text-xs text-slate-400 dark:text-zinc-500 font-mono">
          Engine: rclone {engineStatus?.engine_version || "v1.75.0"} • Port: {engineStatus?.port || 5572}
        </div>

        <button
          onClick={handleSaveSettings}
          className="px-5 py-2.5 bg-blue-600 hover:bg-blue-700 text-white rounded-xl text-xs font-semibold transition shadow-sm flex items-center gap-2"
        >
          {savedSuccess ? <CheckCircle2 className="w-4 h-4 text-white" /> : null}
          {savedSuccess ? "Saved Successfully" : "Save Preferences"}
        </button>
      </div>
    </div>
  );
};
