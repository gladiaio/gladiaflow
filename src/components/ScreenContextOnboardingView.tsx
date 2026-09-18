import { GladiaIcon } from "./GladiaIcon";

const SETTINGS_PATH = "App settings → Use on-screen context for vocabulary";

export function ScreenContextOnboardingView({
  enabled,
  onEnabledChange,
  onContinue,
  isMac,
}: {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  onContinue: () => void;
  isMac: boolean;
}) {
  return (
    <div className="setup-step setup-step-center">
      <h2 className="setup-title">Smarter names from your screen</h2>
      <p className="setup-desc setup-desc--tight">
        GladiaFlow can read on-screen names (for example people in Slack) to
        improve dictation — locally, on your device.
      </p>

      <ul className="setup-benefit-list">
        <li>
          <GladiaIcon name="book" size={16} />
          <span>Proper nouns and product names picked up automatically</span>
        </li>
        <li>
          <GladiaIcon name="microphone" size={16} />
          <span>Better transcription without a manual word list</span>
        </li>
        <li>
          <GladiaIcon name="settings" size={16} />
          <span>
            {isMac
              ? "May ask for Screen Recording so Slack-style apps can be read"
              : "Uses Windows UI Automation and on-device OCR for chat apps"}
          </span>
        </li>
      </ul>

      <div className="setup-opt-in">
        <label className="toggle-switch" title="Enable on-screen vocabulary">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => onEnabledChange(e.target.checked)}
          />
          <span className="toggle-slider" />
        </label>
        <div className="setup-opt-in-copy">
          <strong>Enable on-screen vocabulary</strong>
          <span>
            Turn off anytime in <strong>{SETTINGS_PATH}</strong>.
          </span>
        </div>
      </div>

      <div className="setup-nav setup-nav-center">
        <button type="button" className="btn btn-primary" onClick={onContinue}>
          Continue
        </button>
      </div>
    </div>
  );
}
