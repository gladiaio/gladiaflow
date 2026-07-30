import React, { useRef, useState } from "react";
import { SidebarNav, type NavScreen } from "../components/SidebarNav";
import { PermissionsView } from "../components/PermissionsView";
import { ApiKeyOnboardingView } from "../components/ApiKeyOnboardingView";
import { HomeView } from "../components/HomeView";
import { HistoryView } from "../components/HistoryView";
import { CustomVocabularyView } from "../components/CustomVocabularyView";
import { TranscriptionSettingsView } from "../components/TranscriptionSettingsView";
import { AppSettingsView } from "../components/AppSettingsView";
import { DEFAULT_CUSTOM_VOCABULARY } from "../lib/customVocabulary";
import { DEFAULT_DICTATION_STATS } from "../lib/dictationStats";
import { EUROPEAN_LANGUAGES } from "../lib/languages";
import type { AppSettings, TranscriptionHistoryPage } from "../types";

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
            isMac
            permissions={{ hotkeyError: null }}
            region="EU"
            version="0.1.0"
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
            onDone={noop}
          />
        </AppShell>
      </ScreenFrame>
    </div>
  );
}
