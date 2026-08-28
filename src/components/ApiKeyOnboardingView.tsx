import { invoke } from "@tauri-apps/api/core";
import { ApiKeyField } from "./ApiKeyField";

const API_KEYS_URL = "https://app.gladia.io/apikeys";

export function ApiKeyOnboardingView({
  displayValue,
  isLocked,
  isTesting,
  onChangeKey,
  onSave,
}: {
  displayValue: string;
  isLocked: boolean;
  isTesting: boolean;
  onChangeKey: (v: string) => void;
  onSave: () => void;
}) {
  return (
    <div className="setup-step setup-step-center">
      <h2 className="setup-title">Welcome to GladiaFlow</h2>
      <p className="setup-desc">Enter your Gladia API key to get started.</p>
      <div className="setup-form">
        <ApiKeyField
          displayValue={displayValue}
          isLocked={isLocked}
          isTesting={isTesting}
          onChangeKey={onChangeKey}
          onSave={onSave}
          secondaryAction={
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() =>
                void invoke("open_external_url", { url: API_KEYS_URL })
              }
            >
              Get your API key ↗
            </button>
          }
        />
      </div>
    </div>
  );
}
