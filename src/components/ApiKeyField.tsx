import type { ReactNode } from "react";

export function ApiKeyField({
  displayValue,
  isLocked,
  isTesting,
  onChangeKey,
  onSave,
  secondaryAction,
}: {
  displayValue: string;
  isLocked: boolean;
  isTesting: boolean;
  onChangeKey: (v: string) => void;
  onSave: () => void;
  secondaryAction?: ReactNode;
}) {
  return (
    <div className="setup-form">
      <input
        type="text"
        placeholder="Gladia API Key"
        value={displayValue}
        onChange={(e) => onChangeKey(e.target.value)}
        disabled={isLocked || isTesting}
        className={`api-key-input ${isLocked ? "api-key-input-locked" : ""}`}
        autoCorrect="off"
        autoCapitalize="none"
        autoComplete="off"
        spellCheck={false}
      />
      <div className="setup-nav setup-nav-center">
        <button
          className={`btn ${isLocked ? "btn-ghost" : "btn-primary"}`}
          onClick={onSave}
          disabled={isTesting}
        >
          {isTesting ? (
            <>
              <span className="btn-spinner" aria-hidden="true" />
              Testing...
            </>
          ) : isLocked ? (
            "Change api key"
          ) : (
            "Save API Key"
          )}
        </button>
        {secondaryAction}
      </div>
    </div>
  );
}
