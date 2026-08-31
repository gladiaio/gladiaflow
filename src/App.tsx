import { useEffect, useMemo, useState, useRef, useCallback } from "react";
import type {
  BootstrapState,
  AppStatus,
  AppSettings,
  CustomVocabEntry,
  TranscriptionHistoryPage,
  AudioDeviceInfo,
  AudioDeviceSelection,
  ActivationMode,
} from "./types";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  error as logError,
  info as logInfo,
  warn as logWarn,
} from "@tauri-apps/plugin-log";
import { sendNotification } from "@tauri-apps/plugin-notification";
import { audioCue } from "./lib/audioCue";
import { EUROPEAN_LANGUAGES } from "./lib/languages";
import { countWords, buildDisplayTranscript } from "./lib/transcript";
import {
  captureKeyToken,
  isModifier,
  sortKeys,
  modifierSideToken,
  isSingleModifierToken,
  formatHotkeyLabel,
  formatKeyboardEventForLog,
} from "./lib/keyboardUtils";
import {
  type DictationStats,
  DEFAULT_DICTATION_STATS,
  DICTATION_STATS_STORAGE_KEY,
  getDictationComment,
} from "./lib/dictationStats";
import { pickFunnyHomeMessage } from "./lib/messages";
import { DEFAULT_CUSTOM_VOCABULARY } from "./lib/customVocabulary";
import {
  type AccessibilityState,
  isAccessibilityGranted,
} from "./lib/accessibilityPermission";
import {
  SHORT_EMPTY_DICTATION_LIMIT,
  updateHoldModeWarningStreak,
} from "./lib/holdModeWarning";
import { loadSavedApiKey } from "./lib/configLoad";
import { SidebarNav, type NavScreen } from "./components/SidebarNav";
import { AppSettingsView } from "./components/AppSettingsView";
import { TranscriptionSettingsView } from "./components/TranscriptionSettingsView";
import { PermissionsView } from "./components/PermissionsView";
import { ApiKeyOnboardingView } from "./components/ApiKeyOnboardingView";
import { HomeView } from "./components/HomeView";
import { CustomVocabularyView } from "./components/CustomVocabularyView";
import { HistoryView } from "./components/HistoryView";
import { HISTORY_PAGE_SIZE } from "./components/HistoryView";
import { HoldModeWarningToast } from "./components/HoldModeWarningToast";

type Screen = NavScreen | "permissions" | "api-onboarding";

let notificationPermissionRequest: Promise<boolean> | null = null;

function ensureNotificationPermission(): Promise<boolean> {
  if (notificationPermissionRequest) {
    return notificationPermissionRequest;
  }

  notificationPermissionRequest = (async () => {
    await logInfo(
      "[notifications] checking native notification authorization",
    ).catch(() => {});
    const permissionGranted = await invoke<boolean>(
      "request_notification_permission",
    );
    await (permissionGranted ? logInfo : logWarn)(
      `[notifications] permission ${permissionGranted ? "granted" : "not granted"}`,
    ).catch(() => {});
    return permissionGranted;
  })().finally(() => {
    notificationPermissionRequest = null;
  });

  return notificationPermissionRequest;
}

async function showHoldModeDesktopNotification() {
  const permissionGranted = await ensureNotificationPermission();
  if (permissionGranted) {
    sendNotification({
      title: "Hold dictation mode is active",
      body: "Keep the shortcut held while speaking, or switch to Toggle in App settings.",
    });
    await logInfo("[hold-mode-warning] desktop notification sent").catch(
      () => {},
    );
  } else {
    await logWarn(
      "[hold-mode-warning] desktop notification permission not granted; in-app toast shown only",
    ).catch(() => {});
  }
}

function resolveScreen(
  navScreen: NavScreen,
  permissions: {
    checked: boolean;
    accessibilityState: AccessibilityState | null;
    microphone: string | null;
  },
  hasApiKey: boolean,
): Screen {
  if (
    permissions.checked &&
    (!isAccessibilityGranted(permissions.accessibilityState) ||
      permissions.microphone !== "authorized")
  )
    return "permissions";
  if (!hasApiKey) return "api-onboarding";
  return navScreen;
}

export default function App() {
  const [bootstrap, setBootstrap] = useState<BootstrapState | null>(null);
  const [status, setStatus] = useState<AppStatus>({
    phase: "idle",
    title: "Ready",
    detail: "",
  });
  const [apiKey, setApiKey] = useState<string>("");
  const [isApiKeyLocked, setIsApiKeyLocked] = useState(false);
  const [hasSavedApiKey, setHasSavedApiKey] = useState(false);
  const [isTestingApiKey, setIsTestingApiKey] = useState(false);
  const [configLoadPending, setConfigLoadPending] = useState(true);
  const [configLoadError, setConfigLoadError] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<string>("");
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [navScreen, setNavScreen] = useState<NavScreen>("home");
  const [settings, setSettings] = useState<AppSettings>({
    apiKey: "",
    languages: ["en"],
    activationMode: "push-to-talk",
    audioDevice: "",
    audioDeviceSelection: { mode: "automatic" },
    hotkey: "Fn",
    codeSwitching: false,
    copyToClipboard: false,
    endpointing: 0.1,
    customVocabulary: DEFAULT_CUSTOM_VOCABULARY,
  });
  const [audioDevices, setAudioDevices] = useState<AudioDeviceInfo[]>([]);
  const [platform, setPlatform] = useState<string>("macos");
  const [region, setRegion] = useState<string>("");
  const [appVersion, setAppVersion] = useState<string>("");
  const [languageDropdownOpen, setLanguageDropdownOpen] = useState(false);
  const [activationDropdownOpen, setActivationDropdownOpen] = useState(false);
  const [audioDeviceDropdownOpen, setAudioDeviceDropdownOpen] = useState(false);
  const [dictationStats, setDictationStats] = useState<DictationStats>(
    DEFAULT_DICTATION_STATS,
  );
  const [funnyHomeMessage, setFunnyHomeMessage] = useState<string>(() =>
    pickFunnyHomeMessage(),
  );
  const [historyPageData, setHistoryPageData] =
    useState<TranscriptionHistoryPage | null>(null);
  const [historyQuery, setHistoryQuery] = useState("");
  const [historyPage, setHistoryPage] = useState(1);
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const [languageSearch, setLanguageSearch] = useState("");
  const [permissions, setPermissions] = useState<{
    accessibilityState: AccessibilityState | null;
    microphone: string | null;
    hotkeyError: string | null;
    checked: boolean;
  }>({
    accessibilityState: null,
    microphone: null,
    hotkeyError: null,
    checked: false,
  });
  const [isCapturing, setIsCapturing] = useState(false);
  const [settingsReady, setSettingsReady] = useState(false);
  const [showHoldModeWarning, setShowHoldModeWarning] = useState(false);

  const sessionInitialized = useRef(false);
  const isInitializingRef = useRef(false);
  const pendingStopRef = useRef(false);
  const isCapturingRef = useRef(false);
  const justCapturedRef = useRef(false);
  const languageSettingsLoaded = useRef(false);
  const vocabularySettingsLoaded = useRef(false);
  const endpointingLoaded = useRef(false);
  const copyToClipboardLoaded = useRef(false);
  const activationModeLoaded = useRef(false);
  const audioDeviceSelectionLoaded = useRef(false);

  const isRecordingRef = useRef(false);
  const isProcessingRef = useRef(false);
  const stopRequestedRef = useRef(false);
  const stopFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const earlyAudioStopPromiseRef = useRef<Promise<string | null> | null>(null);
  const earlyReleaseAtRef = useRef<number | null>(null);
  const settingsRef = useRef(settings);
  const permissionsRef = useRef(permissions);
  const checkAllPermissionsRef = useRef<() => void>(() => {});
  const permissionCheckInFlightRef = useRef<Promise<void> | null>(null);
  const apiKeyRef = useRef(apiKey);
  const refreshTranscriptionHistoryRef = useRef<() => Promise<void>>(
    async () => {},
  );
  const latestTranscriptRef = useRef("");
  const finalizedTranscriptRef = useRef("");
  const partialTranscriptRef = useRef("");
  const dictationStartTimeRef = useRef<number | null>(null);
  const completedDictationDurationRef = useRef<number | null>(null);
  const shortEmptyDictationStreakRef = useRef(0);
  const audioReadyRef = useRef(false);
  const activeCaptureIdRef = useRef<string | null>(null);
  const audioReadyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const languageDropdownRef = useRef<HTMLDivElement | null>(null);
  const activationDropdownRef = useRef<HTMLDivElement | null>(null);
  const audioDeviceDropdownRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    ensureNotificationPermission().catch((error) => {
      console.warn("Could not request notification permission:", error);
      logWarn(`[notifications] permission request failed: ${error}`).catch(
        () => {},
      );
    });
  }, []);

  useEffect(() => {
    isRecordingRef.current = isRecording;
  }, [isRecording]);
  useEffect(() => {
    isProcessingRef.current = isProcessing;
  }, [isProcessing]);
  useEffect(() => {
    const activity = isRecording
      ? status.phase === "starting"
        ? "starting"
        : "recording"
      : isProcessing
        ? "finalizing"
        : "idle";
    invoke("set_tray_activity", { activity }).catch(console.error);
  }, [isRecording, isProcessing, status.phase]);
  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);
  useEffect(() => {
    if (!languageSettingsLoaded.current) return;
    invoke("save_language_settings", {
      languages: settings.languages,
      codeSwitching: settings.codeSwitching,
    }).catch(console.error);
    if (sessionInitialized.current && !isRecordingRef.current) {
      sessionInitialized.current = false;
      invoke("close_gladia_session").catch(console.error);
    }
  }, [settings.languages, settings.codeSwitching]);
  useEffect(() => {
    if (!vocabularySettingsLoaded.current) return;
    invoke("save_custom_vocabulary", {
      vocabulary: settings.customVocabulary,
    }).catch(console.error);
    if (sessionInitialized.current && !isRecordingRef.current) {
      sessionInitialized.current = false;
      invoke("close_gladia_session").catch(console.error);
    }
  }, [settings.customVocabulary]);
  useEffect(() => {
    if (!endpointingLoaded.current) return;
    invoke("save_endpointing", { endpointing: settings.endpointing }).catch(
      console.error,
    );
    if (sessionInitialized.current && !isRecordingRef.current) {
      sessionInitialized.current = false;
      invoke("close_gladia_session").catch(console.error);
    }
  }, [settings.endpointing]);
  useEffect(() => {
    if (!copyToClipboardLoaded.current) return;
    invoke("save_copy_to_clipboard", {
      enabled: settings.copyToClipboard,
    }).catch(console.error);
  }, [settings.copyToClipboard]);
  useEffect(() => {
    if (!activationModeLoaded.current) return;
    invoke("save_activation_mode", { mode: settings.activationMode }).catch(
      console.error,
    );
  }, [settings.activationMode]);
  useEffect(() => {
    if (!audioDeviceSelectionLoaded.current) return;
    invoke("save_audio_device_selection", {
      selection: settings.audioDeviceSelection,
    }).catch(console.error);
  }, [settings.audioDeviceSelection]);
  useEffect(() => {
    permissionsRef.current = permissions;
  }, [permissions]);
  useEffect(() => {
    apiKeyRef.current = apiKey;
  }, [apiKey]);

  const clearStopFallbackTimer = () => {
    if (stopFallbackTimerRef.current !== null) {
      clearTimeout(stopFallbackTimerRef.current);
      stopFallbackTimerRef.current = null;
    }
  };

  const clearAudioReadyTimer = () => {
    if (audioReadyTimeoutRef.current !== null) {
      clearTimeout(audioReadyTimeoutRef.current);
      audioReadyTimeoutRef.current = null;
    }
  };

  const setRecordingState = (recording: boolean) => {
    isRecordingRef.current = recording;
    setIsRecording(recording);
  };

  const setProcessingState = (processing: boolean) => {
    isProcessingRef.current = processing;
    setIsProcessing(processing);
  };

  const refreshTranscriptionHistory = useCallback(async () => {
    setIsLoadingHistory(true);
    try {
      const pageData = await invoke<TranscriptionHistoryPage>(
        "list_transcription_history",
        {
          query: historyQuery.trim() || null,
          page: historyPage,
          pageSize: HISTORY_PAGE_SIZE,
        },
      );
      setHistoryPageData(pageData);
    } catch (error) {
      console.error("Failed to load transcription history:", error);
    } finally {
      setIsLoadingHistory(false);
    }
  }, [historyQuery, historyPage]);

  useEffect(() => {
    refreshTranscriptionHistoryRef.current = refreshTranscriptionHistory;
  }, [refreshTranscriptionHistory]);

  useEffect(() => {
    if (navScreen !== "history") return;
    refreshTranscriptionHistory().catch(console.error);
  }, [navScreen, refreshTranscriptionHistory]);

  const handleHistoryQueryChange = useCallback((query: string) => {
    setHistoryQuery(query);
    setHistoryPage(1);
  }, []);

  useEffect(() => {
    try {
      const stored = localStorage.getItem(DICTATION_STATS_STORAGE_KEY);
      if (!stored) return;
      const parsed = JSON.parse(stored) as Partial<DictationStats>;
      const totalWords = Number.isFinite(parsed.totalWords)
        ? Math.max(0, Number(parsed.totalWords))
        : 0;
      const totalSeconds = Number.isFinite(parsed.totalSeconds)
        ? Math.max(0, Number(parsed.totalSeconds))
        : 0;
      setDictationStats({ totalWords, totalSeconds });
    } catch (error) {
      console.warn("Failed to load dictation stats:", error);
    }
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(
        DICTATION_STATS_STORAGE_KEY,
        JSON.stringify(dictationStats),
      );
    } catch (error) {
      console.warn("Failed to persist dictation stats:", error);
    }
  }, [dictationStats]);

  useEffect(() => {
    if (
      !languageDropdownOpen &&
      !activationDropdownOpen &&
      !audioDeviceDropdownOpen
    )
      return;

    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      const clickedLanguage = languageDropdownRef.current?.contains(target);
      const clickedActivation = activationDropdownRef.current?.contains(target);
      const clickedAudioDevice =
        audioDeviceDropdownRef.current?.contains(target);
      if (!clickedLanguage && !clickedActivation && !clickedAudioDevice) {
        setLanguageDropdownOpen(false);
        setActivationDropdownOpen(false);
        setAudioDeviceDropdownOpen(false);
      }
    };

    window.addEventListener("mousedown", onPointerDown);
    return () => window.removeEventListener("mousedown", onPointerDown);
  }, [languageDropdownOpen, activationDropdownOpen, audioDeviceDropdownOpen]);

  const isMac = platform === "macos";

  const DEFAULT_HOTKEY = "Fn";

  const [recordingKeys, setRecordingKeys] = useState<string[]>([]);
  const pressedKeysRef = useRef(new Set<string>());
  const recordedHotkeyRef = useRef<string>("");

  const loadApiKeyFromConfig = useCallback(async () => {
    setConfigLoadPending(true);
    const result = await loadSavedApiKey();
    if (!result.ok) {
      await logError(
        "[config] API key load failed; showing configuration recovery screen",
      ).catch(() => {});
      setConfigLoadError(result.message);
      setConfigLoadPending(false);
      return;
    }

    setConfigLoadError(null);
    if (result.apiKey) {
      setApiKey(result.apiKey);
      setIsApiKeyLocked(true);
      setHasSavedApiKey(true);
    } else {
      setApiKey("");
      setIsApiKeyLocked(false);
      setHasSavedApiKey(false);
    }
    setConfigLoadPending(false);
  }, []);

  useEffect(() => {
    const init = async () => {
      const p = await invoke<string>("get_platform").catch(() => "macos");
      setPlatform(p);

      const r = await invoke<string>("get_region").catch(() => "");
      setRegion(r);

      const v = await getVersion().catch(() => "");
      setAppVersion(v);

      await loadApiKeyFromConfig();

      const savedHotkey = await invoke<string | null>("get_hotkey").catch(
        () => null,
      );
      const hotkey = savedHotkey ?? DEFAULT_HOTKEY;
      if (!savedHotkey) {
        await invoke("save_hotkey", { hotkey }).catch(console.error);
      }

      const [savedLanguages, savedCodeSwitching] = await invoke<
        [string[] | null, boolean | null]
      >("get_language_settings").catch(() => [null, null] as [null, null]);

      const savedVocabulary = await invoke<CustomVocabEntry[]>(
        "get_custom_vocabulary",
      ).catch(() => [] as CustomVocabEntry[]);

      const savedEndpointing = await invoke<number>("get_endpointing").catch(
        () => 0.1,
      );

      const savedCopyToClipboard = await invoke<boolean>(
        "get_copy_to_clipboard",
      ).catch(() => false);

      const savedActivationMode = await invoke<ActivationMode>(
        "get_activation_mode",
      ).catch(() => "push-to-talk" as ActivationMode);

      const savedAudioDeviceSelection = await invoke<AudioDeviceSelection>(
        "get_audio_device_selection",
      ).catch(() => ({ mode: "automatic" }) as AudioDeviceSelection);

      setSettings((prev) => ({
        ...prev,
        hotkey,
        ...(savedLanguages && savedLanguages.length > 0
          ? { languages: savedLanguages }
          : {}),
        ...(savedCodeSwitching !== null
          ? { codeSwitching: savedCodeSwitching }
          : {}),
        endpointing: savedEndpointing,
        copyToClipboard: savedCopyToClipboard,
        activationMode: savedActivationMode,
        audioDeviceSelection: savedAudioDeviceSelection,
        customVocabulary: savedVocabulary,
      }));
      languageSettingsLoaded.current = true;
      vocabularySettingsLoaded.current = true;
      endpointingLoaded.current = true;
      copyToClipboardLoaded.current = true;
      activationModeLoaded.current = true;
      audioDeviceSelectionLoaded.current = true;
      setSettingsReady(true);
    };
    init();
  }, [loadApiKeyFromConfig]);

  useEffect(() => {
    if (!bootstrap) {
      const initialState: BootstrapState = {
        settings: {
          apiKey: apiKey,
          languages: settings.languages,
          activationMode: "push-to-talk",
          audioDevice: "",
          audioDeviceSelection: { mode: "automatic" },
          hotkey: settings.hotkey,
          codeSwitching: settings.codeSwitching,
          copyToClipboard: settings.copyToClipboard,
          endpointing: settings.endpointing,
          customVocabulary: settings.customVocabulary,
        },
        apiKeySet: !!apiKey,
        ready: !!apiKey,
        status,
      };
      setBootstrap(initialState);
      setSettings(initialState.settings);
    }
  }, [apiKey, bootstrap, status]);

  useEffect(() => {
    let cancelled = false;
    let cleanups: (() => void)[] = [];

    const initAudio = () => {
      audioCue.init();
      document.removeEventListener("click", initAudio);
      document.removeEventListener("keydown", initAudio);
    };
    document.addEventListener("click", initAudio);
    document.addEventListener("keydown", initAudio);

    const setup = async () => {
      const unlistenHotkeyPressed = await listen("hotkey-pressed", () => {
        handleHotkeyPressed().catch(console.error);
      });

      const unlistenHotkeyReleased = await listen("hotkey-released", () => {
        handleHotkeyReleased().catch(console.error);
      });

      const unlistenAudioReady = await listen<{
        captureId: string;
        deviceName: string;
        wakeLatencyMs: number;
      }>("audio-capture-ready", (event) => {
        if (
          !event.payload?.captureId ||
          event.payload.captureId !== activeCaptureIdRef.current ||
          pendingStopRef.current ||
          !isRecordingRef.current
        ) {
          return;
        }
        clearAudioReadyTimer();
        audioReadyRef.current = true;
        logInfo(
          `audio ready: device=${event.payload.deviceName} wake-latency=${event.payload.wakeLatencyMs}ms`,
        ).catch(() => {});
        setStatus({
          phase: "listening",
          title: "Listening...",
          detail: "Speak now",
        });
        audioCue.playStartSound();
      });

      const unlistenPartial = await listen<string>(
        "transcription-partial",
        (event) => {
          partialTranscriptRef.current = event.payload;
          const displayText = buildDisplayTranscript(
            finalizedTranscriptRef.current,
            event.payload,
          );
          setTranscript(displayText);
          latestTranscriptRef.current = displayText;
        },
      );

      const unlistenFinal = await listen<string>(
        "transcription-final",
        (event) => {
          finalizedTranscriptRef.current = event.payload;
          partialTranscriptRef.current = "";
          setTranscript(event.payload);
          latestTranscriptRef.current = event.payload;
        },
      );

      const unlistenSessionEnded = await listen<string>(
        "session-ended",
        async (event) => {
          clearStopFallbackTimer();

          if (!stopRequestedRef.current) {
            if (isRecordingRef.current && apiKeyRef.current.trim()) {
              try {
                sessionInitialized.current = false;
                await invoke("close_gladia_session").catch(console.error);
                await invoke("init_gladia_session", {
                  apiKey: apiKeyRef.current.trim(),
                  languages: settingsRef.current.languages,
                  deviceName: null,
                  deviceSelection: settingsRef.current.audioDeviceSelection,
                  codeSwitching: settingsRef.current.codeSwitching,
                  customVocabulary: settingsRef.current.customVocabulary,
                  endpointing: settingsRef.current.endpointing,
                });
                await invoke("subscribe_to_transcriptions");
                sessionInitialized.current = true;
              } catch (error) {
                console.error(
                  "Failed to reconnect transcription session:",
                  error,
                );
                setRecordingState(false);
                setProcessingState(false);
                stopRequestedRef.current = false;
                setStatus({
                  phase: "error",
                  title: "Error",
                  detail: "Session ended unexpectedly",
                });
              }
            }
            return;
          }

          const bestTranscript =
            event.payload.trim().length > 0
              ? event.payload
              : latestTranscriptRef.current;
          const finalText = bestTranscript.trim();
          if (finalText.length > 0) {
            // Pasting + clipboard are handled incrementally in the Rust
            // subscription task; here we only refresh the on-screen display.
            setTranscript(bestTranscript);
            setFunnyHomeMessage((previous) => pickFunnyHomeMessage(previous));
          }
          const dictationStartedAt = dictationStartTimeRef.current;
          if (dictationStartedAt !== null) {
            const elapsedSeconds = Math.max(
              0,
              (Date.now() - dictationStartedAt) / 1000,
            );
            const warningDurationSeconds =
              completedDictationDurationRef.current ?? elapsedSeconds;
            const previousWarningStreak = shortEmptyDictationStreakRef.current;
            const warningResult = updateHoldModeWarningStreak({
              currentStreak: previousWarningStreak,
              activationMode: settingsRef.current.activationMode,
              durationSeconds: warningDurationSeconds,
              transcript: finalText,
            });
            completedDictationDurationRef.current = null;
            shortEmptyDictationStreakRef.current = warningResult.streak;
            if (warningResult.shouldWarn) {
              logInfo(
                `[hold-mode-warning] triggered after ${SHORT_EMPTY_DICTATION_LIMIT} consecutive short empty Hold dictations`,
              ).catch(() => {});
              setShowHoldModeWarning(true);
              showHoldModeDesktopNotification().catch((error) => {
                console.warn("Could not show Hold mode notification:", error);
                logWarn(
                  `[hold-mode-warning] desktop notification failed: ${error}`,
                ).catch(() => {});
              });
            } else if (warningResult.streak > previousWarningStreak) {
              logInfo(
                `[hold-mode-warning] short empty Hold dictation detected (${warningResult.streak}/${SHORT_EMPTY_DICTATION_LIMIT}, ${warningDurationSeconds.toFixed(2)}s)`,
              ).catch(() => {});
            } else if (previousWarningStreak > 0) {
              logInfo(
                `[hold-mode-warning] streak reset (mode=${settingsRef.current.activationMode}, duration=${warningDurationSeconds.toFixed(2)}s, transcript=${finalText.length > 0 ? "present" : "empty"})`,
              ).catch(() => {});
            }
            const dictatedWords = countWords(bestTranscript);
            setDictationStats((prev) => ({
              totalWords: prev.totalWords + dictatedWords,
              totalSeconds: prev.totalSeconds + elapsedSeconds,
            }));
            dictationStartTimeRef.current = null;
          }
          if (sessionInitialized.current) {
            sessionInitialized.current = false;
            await invoke("close_gladia_session").catch(console.error);
          }
        },
      );

      const unlistenPasteComplete = await listen("paste-complete", () => {
        if (!stopRequestedRef.current) {
          return;
        }
        clearStopFallbackTimer();
        stopRequestedRef.current = false;
        setProcessingState(false);
        setStatus({
          phase: "done",
          title: "Ready",
          detail: "Transcription complete",
        });
        refreshTranscriptionHistoryRef.current().catch(console.error);
      });

      const unlistenError = await listen<string>(
        "transcription-error",
        async (event) => {
          const message =
            event.payload?.trim() || "Transcription failed unexpectedly.";
          console.error("Transcription error:", message);
          logError(`Transcription error: ${message}`).catch(() => {});

          isInitializingRef.current = false;
          pendingStopRef.current = false;
          earlyAudioStopPromiseRef.current = null;
          earlyReleaseAtRef.current = null;
          dictationStartTimeRef.current = null;
          stopRequestedRef.current = false;
          clearStopFallbackTimer();
          clearAudioReadyTimer();
          activeCaptureIdRef.current = null;
          audioReadyRef.current = false;
          setRecordingState(false);
          setProcessingState(false);
          await invoke("stop_audio_capture").catch(console.error);
          if (sessionInitialized.current) {
            sessionInitialized.current = false;
            await invoke("close_gladia_session").catch(console.error);
          }
          setStatus({
            phase: "error",
            title: "Transcription failed",
            detail: message,
          });
          audioCue.playErrorSound();
        },
      );

      if (cancelled) {
        unlistenHotkeyPressed();
        unlistenHotkeyReleased();
        unlistenAudioReady();
        unlistenPartial();
        unlistenFinal();
        unlistenSessionEnded();
        unlistenPasteComplete();
        unlistenError();
        return;
      }

      cleanups = [
        unlistenHotkeyPressed,
        unlistenHotkeyReleased,
        unlistenAudioReady,
        unlistenPartial,
        unlistenFinal,
        unlistenSessionEnded,
        unlistenPasteComplete,
        unlistenError,
      ];
    };

    setup();

    return () => {
      cancelled = true;
      clearStopFallbackTimer();
      clearAudioReadyTimer();
      cleanups.forEach((fn) => fn());
      document.removeEventListener("click", initAudio);
      document.removeEventListener("keydown", initAudio);
    };
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isRecording) {
        e.preventDefault();
        handleCancelRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isRecording]);

  useEffect(() => {
    invoke<AudioDeviceInfo[]>("list_audio_device_info")
      .then(setAudioDevices)
      .catch(console.error);
    return () => {
      invoke("unregister_dictation_hotkey").catch(console.error);
    };
  }, []);

  const logHotkey = (message: string) => {
    logInfo(`[hotkey] ${message}`).catch(() => {});
  };

  const applyHotkey = async (
    hotkey: string,
    currentPlatform = platform,
  ): Promise<{ hotkeyError: string | null; appliedHotkey: string }> => {
    logHotkey(`registering dictation hotkey: ${hotkey}`);
    try {
      await invoke("register_dictation_hotkey", { hotkey });
      return { hotkeyError: null, appliedHotkey: hotkey };
    } catch (e) {
      if (hotkey === "Fn" && currentPlatform !== "macos") {
        try {
          await invoke("register_dictation_hotkey", { hotkey: "Ctrl" });
          setSettings((prev) => ({ ...prev, hotkey: "Ctrl" }));
          await invoke("save_hotkey", { hotkey: "Ctrl" }).catch(console.error);
          return {
            hotkeyError:
              "Fn/Globe is not supported on Windows — switched to Ctrl+Space automatically.",
            appliedHotkey: "Ctrl",
          };
        } catch (e2) {
          return { hotkeyError: String(e2), appliedHotkey: hotkey };
        }
      }
      return { hotkeyError: String(e), appliedHotkey: hotkey };
    }
  };

  const runPermissionCheck = async () => {
    let accessibilityState: AccessibilityState = "denied";
    let micStatus = "not_determined";
    let hotkeyError: string | null = null;
    const previous = permissionsRef.current;
    const alreadyOperational =
      previous.checked &&
      isAccessibilityGranted(previous.accessibilityState) &&
      previous.microphone === "authorized" &&
      previous.hotkeyError === null;

    try {
      accessibilityState = await invoke<AccessibilityState>(
        "get_accessibility_state",
      );
    } catch (e) {
      console.error("Accessibility check failed:", e);
    }

    try {
      micStatus = await invoke<string>("check_microphone_permission");
    } catch (e) {
      console.error("Microphone check failed:", e);
    }

    if (
      isAccessibilityGranted(accessibilityState) &&
      micStatus === "authorized"
    ) {
      if (!alreadyOperational) {
        await invoke("unregister_dictation_hotkey").catch(console.error);
        try {
          await invoke<number>("prepare_audio_capture", {
            deviceName: null,
            deviceSelection: settingsRef.current.audioDeviceSelection,
          });
        } catch (error) {
          console.error("Microphone preparation failed:", error);
          hotkeyError = `Microphone preparation failed: ${error}`;
        }
        const hotkey = settingsRef.current.hotkey || DEFAULT_HOTKEY;
        if (!hotkeyError) {
          const result = await applyHotkey(hotkey);
          hotkeyError = result.hotkeyError;
        }
      }
    }

    const nextPermissions = {
      accessibilityState,
      microphone: micStatus,
      hotkeyError,
      checked: true,
    };
    permissionsRef.current = nextPermissions;
    setPermissions(nextPermissions);
  };

  const checkAllPermissions = () => {
    if (!permissionCheckInFlightRef.current) {
      permissionCheckInFlightRef.current = runPermissionCheck().finally(() => {
        permissionCheckInFlightRef.current = null;
      });
    }
    return permissionCheckInFlightRef.current;
  };

  checkAllPermissionsRef.current = checkAllPermissions;

  // Re-check permissions when the window regains focus so granting them in
  // System Settings unlocks the app automatically (no manual "Re-check" click).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (!focused) return;
        // Skip while a dictation is active to avoid re-registering the hotkey.
        if (isCapturingRef.current || isRecordingRef.current) return;
        checkAllPermissionsRef.current();
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(console.error);
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!settingsReady) return;
    checkAllPermissions().catch(console.error);
  }, [settingsReady]);

  useEffect(() => {
    if (!settingsReady || permissions.microphone !== "authorized") return;
    if (!isAccessibilityGranted(permissions.accessibilityState)) return;
    if (isRecordingRef.current || isCapturingRef.current) return;
    const prepareSelectedDevice = async () => {
      await invoke("unregister_dictation_hotkey").catch(console.error);
      try {
        await invoke<number>("prepare_audio_capture", {
          deviceName: null,
          deviceSelection: settings.audioDeviceSelection,
        });
      } finally {
        const result = await applyHotkey(
          settingsRef.current.hotkey || DEFAULT_HOTKEY,
        );
        setPermissions((previous) => ({
          ...previous,
          hotkeyError: result.hotkeyError,
        }));
      }
    };
    prepareSelectedDevice().catch(console.error);
  }, [settings.audioDeviceSelection, settingsReady]);

  const handleHotkeyChange = async (hotkey: string) => {
    setSettings((prev) => ({ ...prev, hotkey }));
    await invoke("save_hotkey", { hotkey }).catch(console.error);
    if (isAccessibilityGranted(permissions.accessibilityState)) {
      await invoke("unregister_dictation_hotkey").catch(console.error);
      const result = await applyHotkey(hotkey);
      setPermissions((prev) => ({ ...prev, hotkeyError: result.hotkeyError }));
    }
  };

  const handleResetHotkey = async () => {
    if (isCapturingRef.current) exitCapture();
    await handleHotkeyChange(DEFAULT_HOTKEY);
  };

  const exitCapture = () => {
    isCapturingRef.current = false;
    setIsCapturing(false);
    setRecordingKeys([]);
    pressedKeysRef.current.clear();
    recordedHotkeyRef.current = "";
    justCapturedRef.current = true;
    setTimeout(() => {
      justCapturedRef.current = false;
    }, 400);
  };

  const startCapture = async () => {
    isCapturingRef.current = true;
    setIsCapturing(true);
    setRecordingKeys([]);
    pressedKeysRef.current.clear();
    recordedHotkeyRef.current = "";

    logHotkey("capture started");
    await invoke("unregister_dictation_hotkey").catch(console.error);
  };

  const cancelCapture = async () => {
    const prevHotkey = settingsRef.current.hotkey;
    logHotkey(`capture cancelled; restoring hotkey=${prevHotkey}`);
    isCapturingRef.current = false;
    setIsCapturing(false);
    setRecordingKeys([]);
    pressedKeysRef.current.clear();

    const result = await applyHotkey(prevHotkey);
    setPermissions((prev) => ({ ...prev, hotkeyError: result.hotkeyError }));
  };

  const saveCapture = async () => {
    const combo = recordedHotkeyRef.current;
    if (!combo) return;

    logHotkey(`capture saved: combo=${combo}`);
    exitCapture();
    setSettings((prev) => ({ ...prev, hotkey: combo }));
    await invoke("save_hotkey", { hotkey: combo }).catch(console.error);
    const result = await applyHotkey(combo);
    setPermissions((prev) => ({ ...prev, hotkeyError: result.hotkeyError }));
  };

  const canSaveCapture = useMemo(() => {
    // A lone modifier (e.g. Right Command) is a valid hotkey on its own — macOS
    // only, since other platforms can't bind a bare modifier natively.
    if (
      isMac &&
      recordingKeys.length === 1 &&
      isSingleModifierToken(recordingKeys[0])
    ) {
      return true;
    }
    const mods = recordingKeys.filter(isModifier);
    const nonMods = recordingKeys.filter((k) => !isModifier(k));
    return mods.length > 0 && nonMods.length > 0;
  }, [recordingKeys, isMac]);

  useEffect(() => {
    if (!isCapturing) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (!isCapturingRef.current) return;
      e.preventDefault();
      e.stopPropagation();

      if (
        e.code === "Escape" &&
        !e.metaKey &&
        !e.shiftKey &&
        !e.ctrlKey &&
        !e.altKey
      ) {
        cancelCapture();
        return;
      }

      const token = captureKeyToken(e);
      const sideToken = modifierSideToken(e.code);
      logHotkey(
        `capture keydown: ${formatKeyboardEventForLog(e)} token=${token ?? "null"} sideToken=${sideToken ?? "null"}`,
      );
      if (!token) return;

      pressedKeysRef.current.add(token);

      const keys = Array.from(pressedKeysRef.current);
      const mods = keys.filter(isModifier);
      const nonMods = keys.filter((k) => !isModifier(k));

      if (mods.length > 0 && nonMods.length > 0) {
        // Composite: modifier(s) + logical key from the current keyboard layout.
        recordedHotkeyRef.current = [...sortKeys(mods), nonMods[0]].join("+");
        setRecordingKeys([...keys]);
      } else if (
        isMac &&
        mods.length === 1 &&
        nonMods.length === 0 &&
        sideToken
      ) {
        // A lone modifier — store the side-qualified token (e.g. Right Command).
        // macOS only: other platforms can't bind a bare modifier, so they keep
        // requiring a composite rather than silently substituting Modifier+Space.
        recordedHotkeyRef.current = sideToken;
        setRecordingKeys([sideToken]);
      } else {
        recordedHotkeyRef.current = "";
        setRecordingKeys([...keys]);
      }

      logHotkey(
        `capture state: pressed=[${keys.join(",")}] recorded=${recordedHotkeyRef.current || "(pending)"}`,
      );
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (!isCapturingRef.current) return;
      e.preventDefault();
      const token = captureKeyToken(e);
      logHotkey(
        `capture keyup: ${formatKeyboardEventForLog(e)} token=${token ?? "null"}`,
      );
      if (token) pressedKeysRef.current.delete(token);
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [isCapturing, isMac]);

  const handleOpenAccessibilitySettings = async () => {
    try {
      await invoke("open_system_settings", { panel: "accessibility" });
    } catch (e) {
      console.error("Failed to open accessibility settings:", e);
    }
  };

  const handleGrantMicrophone = async () => {
    try {
      const granted = await invoke<boolean>("request_microphone_permission");
      if (!granted) {
        await invoke("open_system_settings", { panel: "microphone" });
      }
    } catch (e) {
      console.error("Failed to request microphone:", e);
    }
    await checkAllPermissions();
  };

  const handleOpenMicSettings = async () => {
    try {
      await invoke("open_system_settings", { panel: "microphone" });
    } catch (e) {
      console.error("Failed to open mic settings:", e);
    }
  };

  const handleSaveApiKey = async () => {
    if (isTestingApiKey || !apiKey.trim()) return;

    if (isApiKeyLocked) {
      setIsApiKeyLocked(false);
      setStatus({
        phase: "idle",
        title: "Ready",
        detail: "You can now change your API key.",
      });
      return;
    }

    const trimmedApiKey = apiKey.trim();
    setIsTestingApiKey(true);
    setStatus({
      phase: "transcribing",
      title: "Testing",
      detail: "Checking your Gladia API key...",
    });

    try {
      const connectionOk = await invoke<boolean>("test_gladia_connection", {
        apiKey: trimmedApiKey,
      });
      if (!connectionOk) {
        setStatus({
          phase: "error",
          title: "Error",
          detail: "Invalid API key. Please check and retry.",
        });
        return;
      }

      await invoke("save_api_key", { apiKey: trimmedApiKey });
      setApiKey(trimmedApiKey);
      setIsApiKeyLocked(true);
      setHasSavedApiKey(true);
      setStatus({
        phase: "idle",
        title: "Ready",
        detail: "API key saved and verified!",
      });
      await checkAllPermissions();
    } catch (e) {
      console.error("Failed to save API key:", e);
      setStatus({
        phase: "error",
        title: "Error",
        detail: `Failed to save API key: ${e}`,
      });
    } finally {
      setIsTestingApiKey(false);
    }
  };

  const handleLanguageToggle = (code: string, checked: boolean) => {
    setSettings((prev) => {
      if (checked) {
        if (prev.languages.includes(code)) {
          return prev;
        }
        return { ...prev, languages: [...prev.languages, code] };
      }

      if (prev.languages.length <= 1) {
        return prev;
      }

      return {
        ...prev,
        languages: prev.languages.filter((lang) => lang !== code),
      };
    });
  };

  const handleOpenDropdown = (
    dropdown: "languages" | "activation" | "audioDevice",
  ) => {
    setLanguageDropdownOpen(dropdown === "languages");
    setActivationDropdownOpen(dropdown === "activation");
    setAudioDeviceDropdownOpen(dropdown === "audioDevice");
  };

  const scheduleSessionEndFallback = () => {
    clearStopFallbackTimer();
    stopFallbackTimerRef.current = setTimeout(async () => {
      stopFallbackTimerRef.current = null;
      if (stopRequestedRef.current && sessionInitialized.current) {
        console.warn("Gladia session timed out — closing");
        sessionInitialized.current = false;
        await invoke("close_gladia_session").catch(console.error);
        stopRequestedRef.current = false;
        setProcessingState(false);
        setStatus({
          phase: "done",
          title: "Ready",
          detail: "Transcription complete",
        });
      }
    }, 5_000);
  };

  const handleStartDictation = async () => {
    const keyDownAt = performance.now();
    const captureId = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
    const currentApiKey = apiKeyRef.current;
    if (!currentApiKey.trim()) return;
    if (isProcessingRef.current) return;

    isInitializingRef.current = true;
    pendingStopRef.current = false;
    earlyAudioStopPromiseRef.current = null;
    earlyReleaseAtRef.current = null;
    stopRequestedRef.current = false;
    clearStopFallbackTimer();
    clearAudioReadyTimer();
    activeCaptureIdRef.current = captureId;
    audioReadyRef.current = false;
    latestTranscriptRef.current = "";
    finalizedTranscriptRef.current = "";
    partialTranscriptRef.current = "";
    dictationStartTimeRef.current = Date.now();
    completedDictationDurationRef.current = null;
    setTranscript("");

    setRecordingState(true);
    setProcessingState(false);
    setStatus({
      phase: "starting",
      title: "Starting microphone...",
      detail: "Wait for the sound before speaking",
    });

    audioReadyTimeoutRef.current = setTimeout(async () => {
      audioReadyTimeoutRef.current = null;
      if (activeCaptureIdRef.current !== captureId || audioReadyRef.current) {
        return;
      }
      activeCaptureIdRef.current = null;
      isInitializingRef.current = false;
      pendingStopRef.current = false;
      earlyAudioStopPromiseRef.current = null;
      earlyReleaseAtRef.current = null;
      dictationStartTimeRef.current = null;
      setRecordingState(false);
      setProcessingState(false);
      await invoke("stop_audio_capture").catch(console.error);
      if (sessionInitialized.current) {
        sessionInitialized.current = false;
        await invoke("close_gladia_session").catch(console.error);
      }
      setStatus({
        phase: "error",
        title: "Microphone did not start",
        detail: "Choose another input device in App settings and try again.",
      });
      audioCue.playErrorSound();
    }, 8_000);

    try {
      await invoke("start_audio_capture", {
        deviceName: null,
        deviceSelection: settingsRef.current.audioDeviceSelection,
        captureId,
      });
      if (activeCaptureIdRef.current !== captureId) return;
      logInfo(
        `audio latency: key-down-to-play=${(performance.now() - keyDownAt).toFixed(1)}ms`,
      ).catch(() => {});

      if (!sessionInitialized.current) {
        await invoke("init_gladia_session", {
          apiKey: currentApiKey,
          languages: settingsRef.current.languages,
          deviceName: null,
          deviceSelection: settingsRef.current.audioDeviceSelection,
          codeSwitching: settingsRef.current.codeSwitching,
          customVocabulary: settingsRef.current.customVocabulary,
          endpointing: settingsRef.current.endpointing,
        });
        if (activeCaptureIdRef.current !== captureId) {
          await invoke("close_gladia_session").catch(console.error);
          return;
        }
        await invoke("subscribe_to_transcriptions");
        sessionInitialized.current = true;
      }
    } catch (e) {
      if (activeCaptureIdRef.current !== captureId) return;
      clearAudioReadyTimer();
      activeCaptureIdRef.current = null;
      audioReadyRef.current = false;
      isInitializingRef.current = false;
      pendingStopRef.current = false;
      dictationStartTimeRef.current = null;
      console.error("Failed to start session:", e);
      logError(`Failed to start dictation session: ${e}`).catch(() => {});
      await invoke("stop_audio_capture").catch(console.error);
      earlyAudioStopPromiseRef.current = null;
      earlyReleaseAtRef.current = null;
      setRecordingState(false);
      setProcessingState(false);
      setStatus({
        phase: "error",
        title: "Couldn't start",
        detail: `Failed to start: ${e}`,
      });
      audioCue.playErrorSound();
      return;
    }

    isInitializingRef.current = false;

    if (pendingStopRef.current) {
      pendingStopRef.current = false;
      activeCaptureIdRef.current = null;
      const earlyAudioStopPromise = earlyAudioStopPromiseRef.current;
      const audioStopError = earlyAudioStopPromise
        ? await earlyAudioStopPromise
        : "Early audio stop was not started";
      earlyAudioStopPromiseRef.current = null;
      const earlyReleaseAt = earlyReleaseAtRef.current;
      earlyReleaseAtRef.current = null;

      if (audioStopError) {
        console.error(
          "Error stopping audio after early release:",
          audioStopError,
        );
        logError(
          `Failed to stop dictation after early release: ${audioStopError}`,
        ).catch(() => {});
        dictationStartTimeRef.current = null;
        completedDictationDurationRef.current = null;
        stopRequestedRef.current = false;
        audioCue.playErrorSound();
        sessionInitialized.current = false;
        await invoke("close_gladia_session").catch(console.error);
        setProcessingState(false);
        setStatus({
          phase: "error",
          title: "Couldn't finish",
          detail: `Stop failed: ${audioStopError}`,
        });
        return;
      }

      try {
        await invoke("stop_recording");
        const keyUpToStopSignalMs =
          earlyReleaseAt === null ? null : performance.now() - earlyReleaseAt;
        logInfo(
          `audio latency: key-up-to-stop-signal=${
            keyUpToStopSignalMs === null
              ? "unknown"
              : `${keyUpToStopSignalMs.toFixed(1)}ms`
          } (includes session initialization)`,
        ).catch(() => {});
        scheduleSessionEndFallback();
      } catch (e) {
        console.error("Error finalizing after early release:", e);
        logError(
          `Failed to finalize dictation after early release: ${e}`,
        ).catch(() => {});
        dictationStartTimeRef.current = null;
        completedDictationDurationRef.current = null;
        stopRequestedRef.current = false;
        audioCue.playErrorSound();
        sessionInitialized.current = false;
        await invoke("close_gladia_session").catch(console.error);
        setProcessingState(false);
        setStatus({
          phase: "error",
          title: "Couldn't finish",
          detail: `Stop failed: ${e}`,
        });
      }
    }
  };

  const handleStopDictation = async () => {
    if (
      completedDictationDurationRef.current === null &&
      dictationStartTimeRef.current !== null
    ) {
      completedDictationDurationRef.current = Math.max(
        0,
        (Date.now() - dictationStartTimeRef.current) / 1000,
      );
    }
    const wasAudioReady = audioReadyRef.current;
    clearAudioReadyTimer();
    activeCaptureIdRef.current = null;
    audioReadyRef.current = false;
    stopRequestedRef.current = true;
    clearStopFallbackTimer();
    setRecordingState(false);
    setProcessingState(true);
    setStatus({
      phase: "finalizing",
      title: "Finalizing...",
      detail: "Pasting your transcription",
    });

    if (wasAudioReady) {
      audioCue.playStopSound();
    }

    try {
      await invoke("stop_dictation");
    } catch (e) {
      console.error("Error stopping:", e);
      logError(`Failed to stop dictation session: ${e}`).catch(() => {});
      dictationStartTimeRef.current = null;
      completedDictationDurationRef.current = null;
      stopRequestedRef.current = false;
      clearStopFallbackTimer();
      audioCue.playErrorSound();
      sessionInitialized.current = false;
      await invoke("close_gladia_session").catch(console.error);
      setProcessingState(false);
      setStatus({
        phase: "error",
        title: "Couldn't finish",
        detail: `Stop failed: ${e}`,
      });
      return;
    }

    scheduleSessionEndFallback();
  };

  const handleHotkeyPressed = async () => {
    if (isProcessingRef.current) return;
    if (isCapturingRef.current) {
      exitCapture();
      setSettings((prev) => ({ ...prev, hotkey: "Fn" }));
      await invoke("save_hotkey", { hotkey: "Fn" }).catch(console.error);
      const result = await applyHotkey("Fn");
      setPermissions((prev) => ({ ...prev, hotkeyError: result.hotkeyError }));
      return;
    }
    if (justCapturedRef.current) return;
    if (!apiKeyRef.current.trim()) return;
    if (settingsRef.current.activationMode === "push-to-talk") {
      if (!isRecordingRef.current && !isInitializingRef.current) {
        await handleStartDictation();
      }
    } else {
      if (isRecordingRef.current) {
        await handleStopDictation();
      } else {
        await handleStartDictation();
      }
    }
  };

  const handleHotkeyReleased = async () => {
    if (isProcessingRef.current) return;
    if (isCapturingRef.current) return;
    if (justCapturedRef.current) return;
    if (!apiKeyRef.current.trim()) return;
    if (settingsRef.current.activationMode === "push-to-talk") {
      if (dictationStartTimeRef.current !== null) {
        completedDictationDurationRef.current = Math.max(
          0,
          (Date.now() - dictationStartTimeRef.current) / 1000,
        );
      }
      if (isInitializingRef.current) {
        pendingStopRef.current = true;
        const keyUpAt = performance.now();
        earlyReleaseAtRef.current = keyUpAt;
        const wasAudioReady = audioReadyRef.current;
        clearAudioReadyTimer();
        audioReadyRef.current = false;
        stopRequestedRef.current = true;
        clearStopFallbackTimer();
        setRecordingState(false);
        setProcessingState(true);
        setStatus({
          phase: "finalizing",
          title: "Finalizing...",
          detail: "Pasting your transcription",
        });
        if (wasAudioReady) {
          audioCue.playStopSound();
        }
        logInfo(
          "[dictation-stop] hotkey released during session initialization; stopping audio immediately",
        ).catch(() => {});
        earlyAudioStopPromiseRef.current = invoke("stop_audio_capture")
          .then(() => {
            logInfo(
              `audio latency: key-up-to-capture-paused=${(
                performance.now() - keyUpAt
              ).toFixed(1)}ms`,
            ).catch(() => {});
            return null;
          })
          .catch((error) => String(error));
      } else if (isRecordingRef.current) {
        await handleStopDictation();
      }
    }
  };

  const handleCancelRecording = async () => {
    if (isRecording) {
      clearAudioReadyTimer();
      activeCaptureIdRef.current = null;
      audioReadyRef.current = false;
      isInitializingRef.current = false;
      pendingStopRef.current = false;
      earlyAudioStopPromiseRef.current = null;
      earlyReleaseAtRef.current = null;
      stopRequestedRef.current = false;
      clearStopFallbackTimer();
      dictationStartTimeRef.current = null;
      completedDictationDurationRef.current = null;
      setRecordingState(false);
      setProcessingState(false);
      sessionInitialized.current = false;
      try {
        await invoke("stop_audio_capture");
        await invoke("close_gladia_session");
      } catch (e) {
        console.error("Error canceling:", e);
      }
      setStatus({
        phase: "idle",
        title: "Ready",
        detail: "Recording canceled",
      });
    }
  };

  const languageOptions = EUROPEAN_LANGUAGES;
  const filteredLanguageOptions = useMemo(
    () =>
      languageOptions.filter((language) =>
        language.label
          .toLowerCase()
          .includes(languageSearch.trim().toLowerCase()),
      ),
    [languageOptions, languageSearch],
  );
  const selectedLanguageSummary = useMemo(() => {
    const labels = languageOptions
      .filter((language) => settings.languages.includes(language.code))
      .map((language) => language.label);
    if (labels.length === 0) return "Select languages";
    if (labels.length <= 2) return labels.join(", ");
    return `${labels[0]}, ${labels[1]} +${labels.length - 2}`;
  }, [languageOptions, settings.languages]);
  const activationModeOptions: Array<{
    value: "toggle" | "push-to-talk";
    label: string;
  }> = [
    { value: "toggle", label: "Toggle" },
    { value: "push-to-talk", label: "Hold" },
  ];
  const selectedActivationModeLabel =
    activationModeOptions.find(
      (option) => option.value === settings.activationMode,
    )?.label ?? activationModeOptions[0].label;
  const defaultAudioDevice = audioDevices.find((device) => device.isDefault);
  const automaticAudioDevice =
    defaultAudioDevice?.transport === "bluetooth"
      ? (audioDevices.find((device) => device.isBuiltIn) ?? defaultAudioDevice)
      : defaultAudioDevice;
  const selectedAudioDeviceLabel =
    settings.audioDeviceSelection.mode === "automatic"
      ? `Recommended${automaticAudioDevice ? ` (${automaticAudioDevice.name})` : ""}`
      : settings.audioDeviceSelection.mode === "system_default"
        ? `System Default${defaultAudioDevice ? ` (${defaultAudioDevice.name})` : ""}`
        : settings.audioDeviceSelection.device_name;
  const apiKeyDisplayValue = isApiKeyLocked
    ? "*".repeat(apiKey.length)
    : apiKey;
  const totalMinutes = dictationStats.totalSeconds / 60;
  const totalHours = dictationStats.totalSeconds / 3600;
  const formattedTotalTime =
    totalHours < 5
      ? `${Math.round(totalMinutes).toLocaleString()} min`
      : `${totalHours.toFixed(totalHours >= 10 ? 1 : 2)} h`;
  const funnyDictationComment = getDictationComment(
    dictationStats.totalSeconds,
  );
  const hasPreviousDictation =
    dictationStats.totalWords > 0 || dictationStats.totalSeconds > 0;
  const hotkeyLabel = formatHotkeyLabel(settings.hotkey);
  const defaultIdlePrompt = isMac
    ? "Hold Fn, speak, release."
    : `Use ${hotkeyLabel} to start dictation.`;
  const homeTitle = isRecording
    ? status.phase === "starting"
      ? "Starting microphone..."
      : transcript || "Listening..."
    : isProcessing
      ? transcript || "Finalizing..."
      : "Ready to dictate";
  const homeSubtitle = isProcessing
    ? status.detail || "Pasting your transcription"
    : isRecording
      ? status.phase === "starting"
        ? "Wait for the sound before speaking"
        : null
      : hasPreviousDictation
        ? funnyHomeMessage
        : defaultIdlePrompt;

  const activeScreen = resolveScreen(navScreen, permissions, hasSavedApiKey);
  const isGateScreen =
    activeScreen === "permissions" || activeScreen === "api-onboarding";

  if (configLoadPending || !bootstrap) {
    return (
      <main className="loading-shell">
        <div className="loading-spinner" />
        <span className="loading-text">Loading GladiaFlow</span>
      </main>
    );
  }

  if (configLoadError) {
    return (
      <main className="loading-shell config-error-shell">
        <h2 className="setup-title">Unable to load settings</h2>
        <p className="setup-desc">{configLoadError}</p>
        <div className="setup-nav setup-nav-center">
          <button className="btn btn-primary" onClick={loadApiKeyFromConfig}>
            Retry
          </button>
          <button
            className="btn btn-ghost"
            onClick={() => invoke("open_log_folder").catch(console.error)}
          >
            Open logs folder
          </button>
        </div>
      </main>
    );
  }

  return (
    <div className={`layout${isGateScreen ? " layout-gate" : ""}`}>
      {!isGateScreen && (
        <SidebarNav activeScreen={navScreen} onNavigate={setNavScreen} />
      )}
      <main className="main-content">
        <section className="content">
          {activeScreen === "dictation" && (
            <TranscriptionSettingsView
              settings={settings}
              setSettings={setSettings}
              languageDropdownOpen={languageDropdownOpen}
              setLanguageDropdownOpen={setLanguageDropdownOpen}
              languageDropdownRef={languageDropdownRef}
              handleOpenDropdown={handleOpenDropdown}
              handleLanguageToggle={handleLanguageToggle}
              languageSearch={languageSearch}
              setLanguageSearch={setLanguageSearch}
              filteredLanguageOptions={filteredLanguageOptions}
              selectedLanguageSummary={selectedLanguageSummary}
              onDone={() => setNavScreen("home")}
            />
          )}
          {activeScreen === "settings" && (
            <AppSettingsView
              settings={settings}
              setSettings={setSettings}
              activationDropdownOpen={activationDropdownOpen}
              setActivationDropdownOpen={setActivationDropdownOpen}
              audioDeviceDropdownOpen={audioDeviceDropdownOpen}
              setAudioDeviceDropdownOpen={setAudioDeviceDropdownOpen}
              activationDropdownRef={activationDropdownRef}
              audioDeviceDropdownRef={audioDeviceDropdownRef}
              handleOpenDropdown={handleOpenDropdown}
              activationModeOptions={activationModeOptions}
              selectedActivationModeLabel={selectedActivationModeLabel}
              selectedAudioDeviceLabel={selectedAudioDeviceLabel}
              audioDevices={audioDevices}
              isCapturing={isCapturing}
              recordingKeys={recordingKeys}
              canSaveCapture={canSaveCapture}
              startCapture={startCapture}
              cancelCapture={cancelCapture}
              saveCapture={saveCapture}
              handleResetHotkey={handleResetHotkey}
              defaultHotkey={DEFAULT_HOTKEY}
              isMac={isMac}
              permissions={permissions}
              region={region}
              version={appVersion}
              onDone={() => setNavScreen("home")}
              onOpenLogs={() => invoke("open_log_folder").catch(console.error)}
            />
          )}
          {activeScreen === "vocabulary" && (
            <CustomVocabularyView
              settings={settings}
              setSettings={setSettings}
            />
          )}
          {activeScreen === "history" && (
            <HistoryView
              query={historyQuery}
              onQueryChange={handleHistoryQueryChange}
              page={historyPage}
              onPageChange={setHistoryPage}
              pageData={historyPageData}
              isLoading={isLoadingHistory}
            />
          )}
          {activeScreen === "permissions" && (
            <PermissionsView
              permissions={permissions}
              isMac={isMac}
              onOpenAccessibilitySettings={handleOpenAccessibilitySettings}
              onGrantMicrophone={handleGrantMicrophone}
              onOpenMicSettings={handleOpenMicSettings}
              onRecheck={checkAllPermissions}
            />
          )}
          {activeScreen === "api-onboarding" && (
            <ApiKeyOnboardingView
              displayValue={apiKeyDisplayValue}
              isLocked={isApiKeyLocked}
              isTesting={isTestingApiKey}
              onChangeKey={setApiKey}
              onSave={handleSaveApiKey}
            />
          )}
          {activeScreen === "home" && (
            <HomeView
              isRecording={isRecording}
              isProcessing={isProcessing}
              homeTitle={homeTitle}
              homeSubtitle={homeSubtitle}
              dictationStats={dictationStats}
              formattedTotalTime={formattedTotalTime}
              funnyDictationComment={funnyDictationComment}
              apiKeyDisplayValue={apiKeyDisplayValue}
              isApiKeyLocked={isApiKeyLocked}
              hasSavedApiKey={hasSavedApiKey}
              isTestingApiKey={isTestingApiKey}
              onChangeApiKey={setApiKey}
              onSaveApiKey={handleSaveApiKey}
              permissions={permissions}
              isMac={isMac}
              onRecheck={checkAllPermissions}
              onStopDictation={handleStopDictation}
              errorMessage={status.phase === "error" ? status.detail : null}
              onOpenLogs={() => invoke("open_log_folder").catch(console.error)}
              onDismissError={() =>
                setStatus({ phase: "idle", title: "Ready", detail: "" })
              }
            />
          )}
        </section>
        {showHoldModeWarning && !isRecording && !isProcessing && (
          <HoldModeWarningToast
            onOpenSettings={() => {
              setShowHoldModeWarning(false);
              setNavScreen("settings");
              setActivationDropdownOpen(true);
            }}
            onDismiss={() => setShowHoldModeWarning(false)}
          />
        )}
      </main>
    </div>
  );
}
