import { useEffect, useRef } from "react";

export function ConfigResetDialog({
  open,
  isResetting,
  error,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  isResetting: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    return () => {
      if (dialog.open) dialog.close();
    };
  }, [open]);

  if (!open) return null;

  return (
    <dialog
      ref={dialogRef}
      className="config-reset-dialog"
      aria-labelledby="config-reset-title"
      aria-describedby="config-reset-description"
      onCancel={(event) => {
        event.preventDefault();
        if (!isResetting) onCancel();
      }}
      onMouseDown={(event) => {
        if (isResetting) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        const clickedBackdrop =
          event.clientX < bounds.left ||
          event.clientX > bounds.right ||
          event.clientY < bounds.top ||
          event.clientY > bounds.bottom;
        if (clickedBackdrop) onCancel();
      }}
    >
      <h3 id="config-reset-title">Reset all settings?</h3>
      <p id="config-reset-description">
        This resets your API key, shortcut, languages, vocabulary, and app
        preferences. You will need to set up GladiaFlow again.
      </p>
      <p>
        The unreadable settings file will be kept as a timestamped backup. This
        action only happens if you confirm below.
      </p>
      {error && (
        <p className="text-danger" role="alert">
          {error}
        </p>
      )}
      <div className="config-reset-dialog-actions">
        <button
          type="button"
          className="btn btn-ghost"
          onClick={onCancel}
          disabled={isResetting}
        >
          Cancel
        </button>
        <button
          type="button"
          className="btn btn-danger"
          onClick={onConfirm}
          disabled={isResetting}
        >
          {isResetting ? "Resetting..." : "Reset all settings"}
        </button>
      </div>
    </dialog>
  );
}
