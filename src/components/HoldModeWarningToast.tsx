export function HoldModeWarningToast({
  onOpenSettings,
  onDismiss,
}: {
  onOpenSettings: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="hold-mode-toast" role="alert" aria-live="assertive">
      <div className="hold-mode-toast-icon" aria-hidden="true">
        !
      </div>
      <div className="hold-mode-toast-content">
        <p className="hold-mode-toast-title">Hold mode is active</p>
        <p className="hold-mode-toast-copy">
          Keep the shortcut held while speaking, or{" "}
          <button className="hold-mode-toast-link" onClick={onOpenSettings}>
            switch to Toggle
          </button>
          .
        </p>
      </div>
      <button
        className="hold-mode-toast-close"
        onClick={onDismiss}
        aria-label="Dismiss Hold mode warning"
      >
        ×
      </button>
    </div>
  );
}
