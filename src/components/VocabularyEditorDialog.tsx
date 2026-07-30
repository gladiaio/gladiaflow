import { useEffect, useId, useRef, useState } from "react";
import type { CustomVocabEntry } from "../types";
import { clampVocabIntensity } from "../lib/customVocabulary";
import {
  getIntensityGuidance,
  normalizePronunciationCandidate,
  validateVocabularyTerm,
} from "../lib/vocabularyEditor";

export function VocabularyEditorDialog({
  entry,
  editingIndex,
  entries,
  onSave,
  onDelete,
  onClose,
}: {
  entry: CustomVocabEntry;
  editingIndex: number | null;
  entries: CustomVocabEntry[];
  onSave: (entry: CustomVocabEntry) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const termInputRef = useRef<HTMLInputElement>(null);
  const pronunciationListRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const descriptionId = useId();
  const [draft, setDraft] = useState<CustomVocabEntry>(() => ({
    ...entry,
    pronunciations: [...(entry.pronunciations ?? [])],
  }));
  const [pronunciationInput, setPronunciationInput] = useState("");
  const [pronunciationError, setPronunciationError] = useState<string | null>(
    null,
  );
  const [termError, setTermError] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  const pronunciations = draft.pronunciations ?? [];
  const intensity = clampVocabIntensity(draft.intensity ?? 0.5);
  const guidance = getIntensityGuidance(intensity);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    dialog.showModal();
    window.requestAnimationFrame(() => termInputRef.current?.focus());
    return () => dialog.close();
  }, []);

  const addPronunciation = () => {
    const normalized = normalizePronunciationCandidate(
      pronunciations,
      pronunciationInput,
    );
    if (!pronunciationInput.trim()) return;
    if (!normalized) {
      setPronunciationError("That pronunciation is already listed.");
      return;
    }
    const next = [...pronunciations, normalized];
    setDraft((current) => ({ ...current, pronunciations: next }));
    setPronunciationInput("");
    setPronunciationError(null);
    window.requestAnimationFrame(() => {
      const list = pronunciationListRef.current;
      list?.scrollTo({ top: list.scrollHeight, behavior: "smooth" });
    });
  };

  const removePronunciation = (absoluteIndex: number) => {
    const next = pronunciations.filter((_, index) => index !== absoluteIndex);
    setDraft((current) => {
      if (next.length === 0) {
        const updated = { ...current };
        delete updated.pronunciations;
        return updated;
      }
      return { ...current, pronunciations: next };
    });
    setPronunciationError(null);
  };

  const handleSave = () => {
    const error = validateVocabularyTerm(entries, draft.value, editingIndex);
    if (error) {
      setTermError(error);
      termInputRef.current?.focus();
      return;
    }
    const normalized: CustomVocabEntry = {
      ...draft,
      value: draft.value.trim(),
      intensity,
    };
    if (pronunciations.length > 0) {
      normalized.pronunciations = pronunciations;
    } else {
      delete normalized.pronunciations;
    }
    onSave(normalized);
  };

  return (
    <dialog
      ref={dialogRef}
      className="vocab-editor-dialog"
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onMouseDown={(event) => {
        const bounds = event.currentTarget.getBoundingClientRect();
        const clickedBackdrop =
          event.clientX < bounds.left ||
          event.clientX > bounds.right ||
          event.clientY < bounds.top ||
          event.clientY > bounds.bottom;

        if (clickedBackdrop) onClose();
      }}
    >
      <div className="vocab-editor-header">
        <div>
          <h3 id={titleId}>
            {editingIndex === null
              ? "Add vocabulary term"
              : "Edit vocabulary term"}
          </h3>
          <p id={descriptionId}>Tune how Gladia recognizes this term.</p>
        </div>
        <button
          type="button"
          className="vocab-editor-close"
          onClick={onClose}
          aria-label="Close vocabulary editor"
        >
          ×
        </button>
      </div>

      <div className="vocab-editor-fields">
        <label className="vocab-editor-field">
          <span>Term</span>
          <input
            ref={termInputRef}
            type="text"
            className={`form-input${termError ? " form-input--error" : ""}`}
            value={draft.value}
            onChange={(event) => {
              setDraft((current) => ({
                ...current,
                value: event.target.value,
              }));
              setTermError(null);
            }}
            autoCorrect="off"
            autoCapitalize="none"
            autoComplete="off"
            spellCheck={false}
          />
          <span className="vocab-editor-error" aria-live="polite">
            {termError ?? " "}
          </span>
        </label>

        <div className="vocab-intensity-editor">
          <div className="vocab-editor-label-row">
            <span>Intensity</span>
            <span
              className={`vocab-intensity-badge vocab-intensity-badge--${guidance.tone}`}
            >
              {intensity.toFixed(2)} · {guidance.label}
            </span>
          </div>
          <input
            type="range"
            className="vocab-intensity-slider"
            min={0}
            max={1}
            step={0.05}
            value={intensity}
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                intensity: Number.parseFloat(event.target.value),
              }))
            }
            aria-label="Vocabulary intensity"
          />
          <div className="vocab-intensity-scale" aria-hidden="true">
            <span>0</span>
            <span>Recommended 0.4–0.6</span>
            <span>1</span>
          </div>
          <p
            className={`vocab-intensity-help vocab-intensity-help--${guidance.tone}`}
          >
            {guidance.message}
          </p>
        </div>

        <section className="vocab-pronunciation-editor">
          <div className="vocab-editor-label-row">
            <span>Pronunciations</span>
            <span className="vocab-pronunciation-count">
              {pronunciations.length}{" "}
              {pronunciations.length === 1 ? "variant" : "variants"}
            </span>
          </div>
          <p className="vocab-pronunciation-help">
            Add alternate ways this term may sound when spoken.
          </p>
          <div className="vocab-pronunciation-add">
            <input
              type="text"
              className="form-input"
              value={pronunciationInput}
              placeholder="e.g. gladioflow"
              onChange={(event) => {
                setPronunciationInput(event.target.value);
                setPronunciationError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  addPronunciation();
                }
              }}
              autoCorrect="off"
              autoCapitalize="none"
              autoComplete="off"
              spellCheck={false}
            />
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={addPronunciation}
              disabled={!pronunciationInput.trim()}
            >
              Add
            </button>
          </div>
          <span className="vocab-editor-error" aria-live="polite">
            {pronunciationError ?? " "}
          </span>

          <div ref={pronunciationListRef} className="vocab-pronunciation-list">
            {pronunciations.length > 0 ? (
              pronunciations.map((pronunciation, index) => (
                <div
                  className="vocab-pronunciation-row"
                  key={`${pronunciation}-${index}`}
                >
                  <span>{pronunciation}</span>
                  <button
                    type="button"
                    onClick={() => removePronunciation(index)}
                    aria-label={`Remove pronunciation ${pronunciation}`}
                  >
                    Remove
                  </button>
                </div>
              ))
            ) : (
              <div className="vocab-pronunciation-empty">
                No pronunciation variants yet.
              </div>
            )}
          </div>
        </section>
      </div>

      <div className="vocab-editor-footer">
        {editingIndex !== null &&
          (confirmingDelete ? (
            <div className="vocab-delete-confirmation">
              <span>Delete this term?</span>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => setConfirmingDelete(false)}
              >
                Keep
              </button>
              <button
                type="button"
                className="btn btn-danger btn-sm"
                onClick={onDelete}
              >
                Delete
              </button>
            </div>
          ) : (
            <button
              type="button"
              className="btn btn-ghost btn-sm vocab-delete-trigger"
              onClick={() => setConfirmingDelete(true)}
            >
              Delete term
            </button>
          ))}
        <div className="vocab-editor-footer-actions">
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleSave}
          >
            Save term
          </button>
        </div>
      </div>
    </dialog>
  );
}
