import React, { useRef, useState } from "react";
import { SidebarNav, type NavScreen } from "../components/SidebarNav";
import { PermissionsView } from "../components/PermissionsView";
import { ApiKeyOnboardingView } from "../components/ApiKeyOnboardingView";
import { HomeView } from "../components/HomeView";
import { HistoryView } from "../components/HistoryView";
import { CustomVocabularyView } from "../components/CustomVocabularyView";
import { TranscriptionSettingsView } from "../components/TranscriptionSettingsView";
import { AppSettingsView } from "../components/AppSettingsView";
import { MeetingView } from "../components/MeetingView";
import { MeetingCalendarPrompt } from "../components/MeetingCalendarPrompt";
import { DEFAULT_CUSTOM_VOCABULARY } from "../lib/customVocabulary";
import { DEFAULT_DICTATION_STATS } from "../lib/dictationStats";
import { EUROPEAN_LANGUAGES } from "../lib/languages";
import { DEFAULT_MEETING_PREFERENCES } from "../lib/meetingWorkspace";
import type {
  AppSettings,
  MeetingRecord,
  TranscriptionHistoryPage,
} from "../types";

const noop = () => {};
const MOCK_SETTINGS: AppSettings = {
  apiKey: "sk-********",
  languages: ["en", "fr"],
  activationMode: "push-to-talk",
  audioDevice: "",
  audioDeviceSelection: { mode: "automatic" },
  hotkey: "Fn",
  codeSwitching: false,
  copyToClipboard: false,
  liveTyping: false,
  endpointing: 0.1,
  customVocabulary: [
    ...DEFAULT_CUSTOM_VOCABULARY,
    {
      value: "WebSocket",
      intensity: 0.7,
      pronunciations: [
        "web socket",
        "web-socket",
        "websockit",
        "web soket",
        "double-u socket",
        "web socket protocol",
      ],
    },
    { value: "diarization", intensity: 0.7 },
    { value: "endpointing", intensity: 0.55 },
    { value: "SaaS", intensity: 0.65 },
    { value: "Acme Incorporated", intensity: 0.5 },
    { value: "Gladia API", intensity: 0.8 },
  ],
};

const MOCK_HISTORY: TranscriptionHistoryPage = {
  items: [
    {
      id: "1",
      text: "GladiaFlow makes dictation feel effortless once permissions and your API key are set up.",
      created_at: new Date(Date.now() - 3600000).toISOString(),
    },
    {
      id: "2",
      text: "Custom vocabulary helps with product names like GladiaFlow and internal acronyms.",
      created_at: new Date(Date.now() - 86400000).toISOString(),
    },
    {
      id: "3",
      text: "The fixed history footer keeps navigation controls in a predictable position while moving between pages.",
      created_at: new Date(Date.now() - 172800000).toISOString(),
    },
    {
      id: "4",
      text: "Long transcription previews use two lines so every row remains the same height without introducing a scrollbar in the desktop window.",
      created_at: new Date(Date.now() - 259200000).toISOString(),
    },
  ],
  total: 84,
  page: 1,
  page_size: 4,
};

const MOCK_MEETING: MeetingRecord = {
  id: "meeting-saved",
  title: "Launch planning",
  notes: "Confirm the final approver and share the rollout checklist.",
  summary:
    "The team reviewed next week’s launch plan. Final approval is still required, and Jordan will share the updated rollout checklist.",
  folder: "Product",
  duration: 26 * 60 + 18,
  speakerNames: { owner: "You", "0": "Alex", "1": "Jordan" },
  text: "Let’s review the launch plan for next week. The rollout is ready. We only need final approval. I’ll send the updated checklist after this call.",
  utterances: [
    {
      id: "saved-1",
      text: "Let’s review the launch plan for next week.",
      channel: 0,
      owner: true,
      isFinal: true,
    },
    {
      id: "saved-2",
      text: "The rollout is ready. We only need final approval.",
      channel: 1,
      owner: false,
      speaker: 0,
      isFinal: true,
    },
    {
      id: "saved-3",
      text: "I’ll send the updated checklist after this call.",
      channel: 1,
      owner: false,
      speaker: 1,
      isFinal: true,
    },
  ],
  createdAt: "2026-10-22T12:00:00Z",
  updatedAt: "2026-10-22T12:26:18Z",
};

function AppShell({
  id,
  gate,
  activeScreen = "home",
  children,
}: {
  id: string;
  gate?: boolean;
  activeScreen?: NavScreen;
  children: React.ReactNode;
}) {
  return (
    <div
      id={id}
      data-screenshot={id}
      className={`layout${gate ? " layout-gate" : ""}`}
      style={{ width: 800, height: 600, overflow: "hidden" }}
    >
      {!gate && (
        <SidebarNav activeScreen={activeScreen} onNavigate={noop as never} />
      )}
      <main className="main-content">
        <section className="content">{children}</section>
      </main>
    </div>
  );
}

function ScreenFrame({
  id,
  label,
  children,
}: {
  id: string;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="screenshot-frame-wrap">
      <p className="screenshot-label">{label}</p>
      {children}
    </div>
  );
}

export function ScreenshotGallery() {
  const [settings, setSettings] = useState<AppSettings>(MOCK_SETTINGS);
  const languageDropdownRef = useRef<HTMLDivElement>(null);
  const modelDropdownRef = useRef<HTMLDivElement>(null);
  const activationDropdownRef = useRef<HTMLDivElement>(null);
  const audioDeviceDropdownRef = useRef<HTMLDivElement>(null);
  const filteredLanguageOptions = EUROPEAN_LANGUAGES;

  return (
    <div className="screenshot-gallery">
      <ScreenFrame id="wrap-01-permissions" label="01 Permissions">
        <AppShell id="01-permissions" gate>
          <PermissionsView
            permissions={{
              accessibilityState: "not_determined",
              microphone: "not_determined",
            }}
            isMac
            onOpenAccessibilitySettings={noop}
            onGrantMicrophone={noop}
            onOpenMicSettings={noop}
            onRecheck={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-02-api-key" label="02 API Key">
        <AppShell id="02-api-key" gate>
          <ApiKeyOnboardingView
            displayValue=""
            isLocked={false}
            isTesting={false}
            onChangeKey={noop}
            onSave={noop}
            error={null}
            apiBaseUrlSettings={{
              apiBaseUrl: null,
              effectiveApiBaseUrl: "https://api.gladia.io",
              defaultApiBaseUrl: "https://api.gladia.io",
            }}
            onSaveApiBaseUrl={async () => {}}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-02b-api-key-error" label="02b API Key Error">
        <AppShell id="02b-api-key-error" gate>
          <ApiKeyOnboardingView
            displayValue="gl_invalid_example_key"
            isLocked={false}
            isTesting={false}
            onChangeKey={noop}
            onSave={noop}
            error="Invalid API key for https://eu1.api.gladia.io."
            apiBaseUrlSettings={{
              apiBaseUrl: null,
              effectiveApiBaseUrl: "https://api.gladia.io",
              defaultApiBaseUrl: "https://api.gladia.io",
            }}
            onSaveApiBaseUrl={async () => {}}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-03-home-idle" label="03 Home Idle">
        <AppShell id="03-home-idle">
          <HomeView
            isRecording={false}
            isProcessing={false}
            homeTitle="Ready to dictate"
            homeSubtitle="Hold Fn, speak, release."
            dictationStats={{ totalWords: 1247, totalSeconds: 42 * 60 }}
            formattedTotalTime="42 min"
            funnyDictationComment="You've saved roughly 3 coffee breaks worth of typing."
            apiKeyDisplayValue="****************"
            isApiKeyLocked
            hasSavedApiKey
            isTestingApiKey={false}
            onChangeApiKey={noop}
            onSaveApiKey={noop}
            apiKeyError={null}
            permissions={{ hotkeyError: null }}
            isMac
            onRecheck={noop}
            onStopDictation={noop}
            errorMessage={null}
            onOpenLogs={noop}
            onDismissError={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-04-home-recording" label="04 Home Recording">
        <AppShell id="04-home-recording">
          <HomeView
            isRecording
            isProcessing={false}
            homeTitle="Listening..."
            homeSubtitle={null}
            dictationStats={DEFAULT_DICTATION_STATS}
            formattedTotalTime="0 min"
            funnyDictationComment=""
            apiKeyDisplayValue=""
            isApiKeyLocked
            hasSavedApiKey
            isTestingApiKey={false}
            onChangeApiKey={noop}
            onSaveApiKey={noop}
            apiKeyError={null}
            permissions={{ hotkeyError: null }}
            isMac
            onRecheck={noop}
            onStopDictation={noop}
            errorMessage={null}
            onOpenLogs={noop}
            onDismissError={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-04b-home-api-open" label="04b Home API Open">
        <AppShell id="04b-home-api-open">
          <HomeView
            isRecording={false}
            isProcessing={false}
            homeTitle="Ready to dictate"
            homeSubtitle="Hold Fn, speak, release."
            dictationStats={{ totalWords: 1247, totalSeconds: 42 * 60 }}
            formattedTotalTime="42 min"
            funnyDictationComment="You've saved roughly 3 coffee breaks worth of typing."
            apiKeyDisplayValue="sk-example"
            isApiKeyLocked={false}
            hasSavedApiKey
            isTestingApiKey={false}
            onChangeApiKey={noop}
            onSaveApiKey={noop}
            apiKeyError={null}
            permissions={{ hotkeyError: null }}
            isMac
            onRecheck={noop}
            onStopDictation={noop}
            errorMessage={null}
            onOpenLogs={noop}
            onDismissError={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05-history" label="05 History">
        <AppShell id="05-history" activeScreen="history">
          <HistoryView
            query=""
            onQueryChange={noop}
            page={1}
            onPageChange={noop}
            pageData={MOCK_HISTORY}
            isLoading={false}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05d-meeting-idle" label="05d Meeting Idle">
        <AppShell id="05d-meeting-idle" activeScreen="meeting">
          <MeetingView
            phase="idle"
            systemAudioCapability={{
              backend: "core_audio_process_tap",
              permission: "authorized",
              requiresRestart: false,
            }}
            utterances={[]}
            meetings={[]}
            selectedMeeting={null}
            loading={false}
            calendar={{ status: "not_determined", meetings: [] }}
            calendarLoading={false}
            error={null}
            liveTitle=""
            liveNotes=""
            preferences={DEFAULT_MEETING_PREFERENCES}
            onPreferencesChange={noop}
            onLiveTitleChange={noop}
            onLiveNotesChange={noop}
            onStart={noop}
            onStartFromCalendar={noop}
            onConnectCalendar={noop}
            onOpenCalendarMeeting={noop}
            onStop={noop}
            onPause={noop}
            onResume={noop}
            onSelectMeeting={noop}
            onUpdateMeeting={async () => {
              throw new Error("Not available in screenshots");
            }}
            onDeleteMeeting={async () => {}}
            onExportMarkdown={async () => ""}
            onSaveMarkdown={async () => false}
            onGrantPermission={noop}
            onOpenSettings={noop}
            onRecheckPermission={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05e-meeting-live" label="05e Meeting Live">
        <AppShell id="05e-meeting-live" activeScreen="meeting">
          <MeetingView
            phase="recording"
            systemAudioCapability={{
              backend: "core_audio_process_tap",
              permission: "authorized",
              requiresRestart: false,
            }}
            utterances={[
              {
                id: "meeting-1",
                text: "Let’s review the launch plan for next week.",
                channel: 0,
                owner: true,
                isFinal: true,
              },
              {
                id: "meeting-2",
                text: "The rollout is ready. We only need final approval.",
                channel: 1,
                owner: false,
                speaker: 0,
                isFinal: true,
              },
              {
                id: "meeting-3",
                text: "I’ll send the updated checklist after this call.",
                channel: 1,
                owner: false,
                speaker: 1,
                isFinal: false,
              },
            ]}
            meetings={[]}
            selectedMeeting={null}
            loading={false}
            calendar={null}
            calendarLoading={false}
            error={null}
            liveTitle="Launch planning"
            liveNotes=""
            preferences={DEFAULT_MEETING_PREFERENCES}
            onPreferencesChange={noop}
            onLiveTitleChange={noop}
            onLiveNotesChange={noop}
            onStart={noop}
            onStartFromCalendar={noop}
            onConnectCalendar={noop}
            onOpenCalendarMeeting={noop}
            onStop={noop}
            onPause={noop}
            onResume={noop}
            onSelectMeeting={noop}
            onUpdateMeeting={async () => {
              throw new Error("Not available in screenshots");
            }}
            onDeleteMeeting={async () => {}}
            onExportMarkdown={async () => ""}
            onSaveMarkdown={async () => false}
            onGrantPermission={noop}
            onOpenSettings={noop}
            onRecheckPermission={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05f-meeting-saved" label="05f Saved Meeting">
        <AppShell id="05f-meeting-saved" activeScreen="meeting">
          <MeetingView
            phase="idle"
            systemAudioCapability={{
              backend: "core_audio_process_tap",
              permission: "authorized",
              requiresRestart: false,
            }}
            utterances={[]}
            meetings={[MOCK_MEETING]}
            selectedMeeting={MOCK_MEETING}
            loading={false}
            calendar={null}
            calendarLoading={false}
            error={null}
            liveTitle=""
            liveNotes=""
            preferences={{
              ...DEFAULT_MEETING_PREFERENCES,
              defaultSavedMeetingTab: "summary",
            }}
            onPreferencesChange={noop}
            onLiveTitleChange={noop}
            onLiveNotesChange={noop}
            onStart={noop}
            onStartFromCalendar={noop}
            onConnectCalendar={noop}
            onOpenCalendarMeeting={noop}
            onStop={noop}
            onPause={noop}
            onResume={noop}
            onSelectMeeting={noop}
            onUpdateMeeting={async () => MOCK_MEETING}
            onDeleteMeeting={async () => {}}
            onExportMarkdown={async () => ""}
            onSaveMarkdown={async () => false}
            onGrantPermission={noop}
            onOpenSettings={noop}
            onRecheckPermission={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05g-meeting-prompt" label="05g Meeting Prompt">
        <AppShell id="05g-meeting-prompt" activeScreen="home">
          <div className="meeting-empty">
            <strong>Current meeting reminder</strong>
            <span>Shown from the local macOS Calendar.</span>
          </div>
          <MeetingCalendarPrompt
            meeting={{
              id: "calendar-retro",
              title: "Retro",
              startAt: "2026-10-22T14:00:00Z",
              endAt: "2026-10-22T15:00:00Z",
              joinUrl: "https://meet.example.com/retro",
            }}
            onJoinAndStart={noop}
            onStart={noop}
            onDismiss={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05b-history-loading" label="05b History Loading">
        <AppShell id="05b-history-loading" activeScreen="history">
          <HistoryView
            query=""
            onQueryChange={noop}
            page={1}
            onPageChange={noop}
            pageData={null}
            isLoading
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-05c-history-empty" label="05c History Empty">
        <AppShell id="05c-history-empty" activeScreen="history">
          <HistoryView
            query="missing"
            onQueryChange={noop}
            page={1}
            onPageChange={noop}
            pageData={{ items: [], total: 0, page: 1, page_size: 4 }}
            isLoading={false}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-06-vocabulary" label="06 Vocabulary">
        <AppShell id="06-vocabulary" activeScreen="vocabulary">
          <CustomVocabularyView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame
        id="wrap-06b-vocabulary-editor"
        label="06b Vocabulary Editor"
      >
        <AppShell id="06b-vocabulary-editor" activeScreen="vocabulary">
          <CustomVocabularyView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-06c-vocabulary-new" label="06c New Vocabulary Term">
        <AppShell id="06c-vocabulary-new" activeScreen="vocabulary">
          <CustomVocabularyView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame
        id="wrap-07-transcription-settings"
        label="07 Transcription Settings"
      >
        <AppShell id="07-transcription-settings" activeScreen="dictation">
          <TranscriptionSettingsView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
            languageDropdownOpen={false}
            setLanguageDropdownOpen={noop}
            languageDropdownRef={languageDropdownRef}
            handleOpenDropdown={noop}
            handleLanguageToggle={noop}
            languageSearch=""
            setLanguageSearch={noop}
            filteredLanguageOptions={filteredLanguageOptions}
            selectedLanguageSummary="English, French"
            modelDropdownOpen={false}
            setModelDropdownOpen={noop}
            modelDropdownRef={modelDropdownRef}
            liveModel="solaria-3"
            liveModelOptions={{
              status: "ready",
              models: ["trantoria-1", "solaria-1", "solaria-3"],
              discovered: true,
            }}
            apiHost="eu1.api.gladia.io"
            liveTypingWaitsForRelease={false}
            onSelectLiveModel={noop}
            onDone={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-08-app-settings" label="08 App Settings">
        <AppShell id="08-app-settings" activeScreen="settings">
          <AppSettingsView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
            activationDropdownOpen={false}
            setActivationDropdownOpen={noop}
            audioDeviceDropdownOpen={false}
            setAudioDeviceDropdownOpen={noop}
            activationDropdownRef={activationDropdownRef}
            audioDeviceDropdownRef={audioDeviceDropdownRef}
            handleOpenDropdown={noop}
            activationModeOptions={[
              { value: "toggle", label: "Toggle" },
              { value: "push-to-talk", label: "Hold" },
            ]}
            selectedActivationModeLabel="Hold"
            selectedAudioDeviceLabel="System Default"
            audioDevices={[
              {
                id: "built-in",
                name: "MacBook Pro Microphone",
                transport: "built_in",
                isDefault: true,
                isBuiltIn: true,
              },
            ]}
            isCapturing={false}
            recordingKeys={[]}
            canSaveCapture={false}
            startCapture={noop}
            cancelCapture={noop}
            saveCapture={noop}
            handleResetHotkey={noop}
            defaultHotkey="Fn"
            permissions={{ hotkeyError: null }}
            region="EU"
            version="0.1.0"
            apiBaseUrlSettings={{
              apiBaseUrl: null,
              effectiveApiBaseUrl: "https://api.gladia.io",
              defaultApiBaseUrl: "https://api.gladia.io",
            }}
            onSaveApiBaseUrl={async () => {}}
            onDone={noop}
            onOpenLogs={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame
        id="wrap-08b-app-settings-developer"
        label="08b App Settings (Custom API URL)"
      >
        <AppShell id="08b-app-settings-developer" activeScreen="settings">
          <AppSettingsView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
            activationDropdownOpen={false}
            setActivationDropdownOpen={noop}
            audioDeviceDropdownOpen={false}
            setAudioDeviceDropdownOpen={noop}
            activationDropdownRef={activationDropdownRef}
            audioDeviceDropdownRef={audioDeviceDropdownRef}
            handleOpenDropdown={noop}
            activationModeOptions={[
              { value: "toggle", label: "Toggle" },
              { value: "push-to-talk", label: "Hold" },
            ]}
            selectedActivationModeLabel="Hold"
            selectedAudioDeviceLabel="System Default"
            audioDevices={[
              {
                id: "built-in",
                name: "MacBook Pro Microphone",
                transport: "built_in",
                isDefault: true,
                isBuiltIn: true,
              },
            ]}
            isCapturing={false}
            recordingKeys={[]}
            canSaveCapture={false}
            startCapture={noop}
            cancelCapture={noop}
            saveCapture={noop}
            handleResetHotkey={noop}
            defaultHotkey="Fn"
            permissions={{ hotkeyError: null }}
            region="EU"
            version="0.1.0"
            apiBaseUrlSettings={{
              apiBaseUrl: "http://localhost:8080",
              effectiveApiBaseUrl: "http://localhost:8080",
              defaultApiBaseUrl: "https://api.gladia.io",
            }}
            onSaveApiBaseUrl={async () => {}}
            onDone={noop}
            onOpenLogs={noop}
          />
        </AppShell>
      </ScreenFrame>

      <ScreenFrame id="wrap-09-language-picker" label="09 Language Picker">
        <AppShell id="09-language-picker" activeScreen="dictation">
          <TranscriptionSettingsView
            settings={settings}
            setSettings={
              setSettings as React.Dispatch<React.SetStateAction<AppSettings>>
            }
            languageDropdownOpen
            setLanguageDropdownOpen={noop}
            languageDropdownRef={languageDropdownRef}
            handleOpenDropdown={noop}
            handleLanguageToggle={noop}
            languageSearch=""
            setLanguageSearch={noop}
            filteredLanguageOptions={filteredLanguageOptions}
            selectedLanguageSummary="English, French"
            modelDropdownOpen={false}
            setModelDropdownOpen={noop}
            modelDropdownRef={modelDropdownRef}
            liveModel="solaria-3"
            liveModelOptions={{
              status: "ready",
              models: ["trantoria-1", "solaria-1", "solaria-3"],
              discovered: true,
            }}
            apiHost="eu1.api.gladia.io"
            liveTypingWaitsForRelease={false}
            onSelectLiveModel={noop}
            onDone={noop}
          />
        </AppShell>
      </ScreenFrame>
    </div>
  );
}
