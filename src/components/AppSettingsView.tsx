import type { AppSettings, AudioDeviceInfo } from "../types";
import {
  formatKeySymbol,
  sortKeys,
  parseHotkeyParts,
} from "../lib/keyboardUtils";
import { InfoTooltip } from "./InfoTooltip";
import { SettingsPageLayout } from "./SettingsPageLayout";

export function AppSettingsView({
  settings,
  setSettings,
  activationDropdownOpen,
  setActivationDropdownOpen,
  audioDeviceDropdownOpen,
  setAudioDeviceDropdownOpen,
  activationDropdownRef,
  audioDeviceDropdownRef,
  handleOpenDropdown,
  activationModeOptions,
  selectedActivationModeLabel,
  selectedAudioDeviceLabel,
  audioDevices,
  isCapturing,
  recordingKeys,
  canSaveCapture,
  startCapture,
  cancelCapture,
  saveCapture,
  handleResetHotkey,
  defaultHotkey,
  isMac,
  permissions,
  region,
  version,
  onDone,
  onOpenLogs,
}: {
  settings: AppSettings;
  setSettings: React.Dispatch<React.SetStateAction<AppSettings>>;
  activationDropdownOpen: boolean;
  setActivationDropdownOpen: (v: boolean) => void;
  audioDeviceDropdownOpen: boolean;
  setAudioDeviceDropdownOpen: (v: boolean) => void;
  activationDropdownRef: React.RefObject<HTMLDivElement | null>;
  audioDeviceDropdownRef: React.RefObject<HTMLDivElement | null>;
  handleOpenDropdown: (d: "languages" | "activation" | "audioDevice") => void;
  activationModeOptions: Array<{
    value: "toggle" | "push-to-talk";
    label: string;
  }>;
  selectedActivationModeLabel: string;
  selectedAudioDeviceLabel: string;
  audioDevices: AudioDeviceInfo[];
  isCapturing: boolean;
  recordingKeys: string[];
  canSaveCapture: boolean;
  startCapture: () => void;
  cancelCapture: () => void;
  saveCapture: () => void;
  handleResetHotkey: () => void;
  defaultHotkey: string;
  isMac: boolean;
  permissions: { hotkeyError: string | null };
  region: string;
  version: string;
  onDone: () => void;
  onOpenLogs: () => void;
}) {
  const renderKbdKeys = (keys: string[]) =>
    sortKeys(keys).map((key, i) => (
      <kbd key={i} className="shortcut-kbd">
        {formatKeySymbol(key)}
      </kbd>
    ));

  const regionLabel = region || null;

  const currentHotkeyParts = parseHotkeyParts(settings.hotkey);
  const defaultDevice = audioDevices.find((device) => device.isDefault);
  const builtInDevice = audioDevices.find((device) => device.isBuiltIn);
  const automaticDevice =
    defaultDevice?.transport === "bluetooth"
      ? (builtInDevice ?? defaultDevice)
      : defaultDevice;

  return (
    <SettingsPageLayout
      title="App settings"
      footer={
        <p className="settings-region-footer">
          {regionLabel ? `Region: ${regionLabel} · ` : ""}
          {version ? `v${version}` : ""}
        </p>
      }
    >
      <div className="settings-row">
        <div className="form-group">
          <label className="form-label">
            Trigger key
            <InfoTooltip label="Shortcut rules">
              <strong>Rules for setting a shortcut:</strong>
              <ul>
                <li>A modifier (⌘ ⌃ ⌥ ⇧) + a regular key</li>
                {isMac && (
                  <li>A single modifier (left and right are distinct)</li>
                )}
                <li>Press Escape to cancel</li>
              </ul>
            </InfoTooltip>
          </label>
          <div
            className={`shortcut-recorder${isCapturing ? " recording" : ""}`}
          >
            {isCapturing ? (
              <>
                <div className="shortcut-keys recording">
                  {recordingKeys.length > 0 ? (
                    renderKbdKeys(recordingKeys)
                  ) : (
                    <span className="recording-hint">Press shortcut…</span>
                  )}
                </div>
                <div className="shortcut-actions">
                  <button
                    className="btn btn-primary btn-sm"
                    onClick={saveCapture}
                    disabled={!canSaveCapture}
                  >
                    Save
                  </button>
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={cancelCapture}
                  >
                    Cancel
                  </button>
                </div>
              </>
            ) : (
              <>
                <div className="shortcut-keys">
                  {renderKbdKeys(currentHotkeyParts)}
                </div>
                <button className="btn btn-ghost btn-sm" onClick={startCapture}>
                  Edit
                </button>
                {settings.hotkey !== defaultHotkey && (
                  <button
                    className="hotkey-reset-btn"
                    onClick={handleResetHotkey}
                    title="Reset to Fn / Globe"
                    aria-label="Reset trigger key to Fn / Globe"
                  >
                    <span className="hotkey-reset-icon">↻</span>
                  </button>
                )}
              </>
            )}
          </div>
          {permissions.hotkeyError && (
            <p className="form-footnote form-footnote--danger">
              {permissions.hotkeyError}
            </p>
          )}
        </div>
        <div className="form-group">
          <label className="form-label">Activation mode</label>
          <div
            className={`multi-select ${activationDropdownOpen ? "open" : ""}`}
            ref={activationDropdownRef}
          >
            <button
              type="button"
              className="form-input multi-select-trigger"
              onClick={() => {
                if (activationDropdownOpen) {
                  setActivationDropdownOpen(false);
                } else {
                  handleOpenDropdown("activation");
                }
              }}
              aria-haspopup="listbox"
              aria-expanded={activationDropdownOpen}
            >
              <span>{selectedActivationModeLabel}</span>
            </button>
            {activationDropdownOpen && (
              <div className="multi-select-dropdown">
                <div className="multi-select-options" role="listbox">
                  {activationModeOptions.map((option) => {
                    const selected = settings.activationMode === option.value;
                    return (
                      <button
                        key={option.value}
                        type="button"
                        className={`dropdown-option ${selected ? "selected" : ""}`}
                        onClick={() => {
                          setSettings({
                            ...settings,
                            activationMode: option.value,
                          });
                          setActivationDropdownOpen(false);
                        }}
                      >
                        {option.label}
                      </button>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      <div className="form-group">
        <label className="form-label">
          Copy to clipboard
          <InfoTooltip label="About copy to clipboard">
            <strong>Copy to clipboard</strong>
            When on, the full transcription is left on your clipboard after each
            dictation.
          </InfoTooltip>
        </label>
        <label className="toggle-switch" title="Copy to clipboard">
          <input
            type="checkbox"
            checked={settings.copyToClipboard}
            onChange={(e) =>
              setSettings({
                ...settings,
                copyToClipboard: e.target.checked,
              })
            }
          />
          <span className="toggle-slider" />
        </label>
      </div>

      <div className="form-group">
        <label className="form-label">
          Use on-screen context for vocabulary
          <InfoTooltip label="About on-screen vocabulary">
            <strong>On-screen context</strong>
            When on, GladiaFlow samples window titles and Accessibility / UI
            Automation text into a rolling vocabulary store. For Slack and
            similar Electron apps (where accessibility text stays empty), it
            also OCRs those windows — grant Screen Recording (macOS) when
            prompted. Manual vocabulary always wins.
          </InfoTooltip>
        </label>
        <label
          className="toggle-switch"
          title="Use on-screen context for vocabulary"
        >
          <input
            type="checkbox"
            checked={settings.useScreenContextVocabulary}
            onChange={(e) =>
              setSettings({
                ...settings,
                useScreenContextVocabulary: e.target.checked,
              })
            }
          />
          <span className="toggle-slider" />
        </label>
      </div>

      <div className="form-group">
        <label className="form-label">
          Screen OCR for vocabulary
          <InfoTooltip label="About screen OCR vocabulary">
            <strong>Screen OCR</strong>
            Optional full-display OCR fallback. When on (macOS), GladiaFlow OCRs your
            displays if Accessibility text is thin. On Windows, chat windows
            (Slack, …) are OCR'd automatically when screen-context is on.
            Requires Screen Recording on macOS. Default off.
          </InfoTooltip>
        </label>
        <label
          className="toggle-switch"
          title="Use screen OCR for vocabulary"
        >
          <input
            type="checkbox"
            checked={settings.useScreenContextOcr}
            disabled={!settings.useScreenContextVocabulary}
            onChange={(e) =>
              setSettings({
                ...settings,
                useScreenContextOcr: e.target.checked,
              })
            }
          />
          <span className="toggle-slider" />
        </label>
      </div>

      <div className="form-group">
        <label className="form-label">
          Input audio device
          <InfoTooltip label="About input audio devices">
            Use built-in microphone for an optimal experience
          </InfoTooltip>
        </label>
        <div
          className={`multi-select ${audioDeviceDropdownOpen ? "open" : ""}`}
          ref={audioDeviceDropdownRef}
        >
          <button
            type="button"
            className="form-input multi-select-trigger"
            onClick={() => {
              if (audioDeviceDropdownOpen) {
                setAudioDeviceDropdownOpen(false);
              } else {
                handleOpenDropdown("audioDevice");
              }
            }}
            aria-haspopup="listbox"
            aria-expanded={audioDeviceDropdownOpen}
          >
            <span>{selectedAudioDeviceLabel}</span>
          </button>
          {audioDeviceDropdownOpen && (
            <div className="multi-select-dropdown">
              <div className="multi-select-options" role="listbox">
                <button
                  type="button"
                  className={`dropdown-option ${settings.audioDeviceSelection.mode === "automatic" ? "selected" : ""}`}
                  onClick={() => {
                    setSettings({
                      ...settings,
                      audioDevice: "",
                      audioDeviceSelection: { mode: "automatic" },
                    });
                    setAudioDeviceDropdownOpen(false);
                  }}
                >
                  Recommended
                  {automaticDevice ? ` (${automaticDevice.name})` : ""}
                </button>
                <button
                  type="button"
                  className={`dropdown-option ${settings.audioDeviceSelection.mode === "system_default" ? "selected" : ""}`}
                  onClick={() => {
                    setSettings({
                      ...settings,
                      audioDevice: "",
                      audioDeviceSelection: { mode: "system_default" },
                    });
                    setAudioDeviceDropdownOpen(false);
                  }}
                >
                  System Default
                  {defaultDevice ? ` (${defaultDevice.name})` : ""}
                </button>
                {audioDevices.map((device) => {
                  const selected =
                    settings.audioDeviceSelection.mode === "specific" &&
                    settings.audioDeviceSelection.device_id === device.id;
                  return (
                    <button
                      key={device.id || device.name}
                      type="button"
                      className={`dropdown-option ${selected ? "selected" : ""}`}
                      onClick={() => {
                        setSettings({
                          ...settings,
                          audioDevice: device.name,
                          audioDeviceSelection: {
                            mode: "specific",
                            device_id: device.id,
                            device_name: device.name,
                          },
                        });
                        setAudioDeviceDropdownOpen(false);
                      }}
                    >
                      {device.name}
                    </button>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </div>

      <div className="form-group">
        <label className="form-label">Diagnostics</label>
        <p className="setup-desc setup-desc--tight">
          If transcription fails, open the logs folder and share the log file so
          the issue can be analyzed.
        </p>
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          onClick={onOpenLogs}
        >
          Open logs folder
        </button>
      </div>

      <div className="setup-nav setup-nav-center">
        <button className="btn btn-primary" onClick={onDone}>
          Done
        </button>
      </div>
    </SettingsPageLayout>
  );
}
