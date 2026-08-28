import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { AppSettings, CustomVocabEntry } from "../types";
import {
  DEFAULT_VOCAB_INTENSITY,
  downloadVocabularyExport,
  importVocabularyFromFile,
  mergeVocabulary,
} from "../lib/customVocabulary";
import { clampPage } from "../lib/vocabularyEditor";
import { VocabularyEditorDialog } from "./VocabularyEditorDialog";

const DOCS_URL =
  "https://docs.gladia.io/chapters/audio-intelligence/custom-vocabulary";
const VOCABULARY_PAGE_SIZE = 5;

type EditorState = {
  index: number | null;
  entry: CustomVocabEntry;
};

export function CustomVocabularyView({
  settings,
  setSettings,
}: {
  settings: AppSettings;
  setSettings: React.Dispatch<React.SetStateAction<AppSettings>>;
}) {
  const [currentPage, setCurrentPage] = useState(1);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const vocabularyLength = settings.customVocabulary.length;
  const totalPages = Math.max(
    1,
    Math.ceil(vocabularyLength / VOCABULARY_PAGE_SIZE),
  );
  const pageStart = (currentPage - 1) * VOCABULARY_PAGE_SIZE;
  const visibleEntries = settings.customVocabulary.slice(
    pageStart,
    pageStart + VOCABULARY_PAGE_SIZE,
  );

  useEffect(() => {
    setCurrentPage((page) =>
      clampPage(page, vocabularyLength, VOCABULARY_PAGE_SIZE),
    );
  }, [vocabularyLength]);

  const openEditor = (state: EditorState, trigger: HTMLElement) => {
    returnFocusRef.current = trigger;
    setEditor(state);
  };

  const closeEditor = () => {
    setEditor(null);
    window.requestAnimationFrame(() => returnFocusRef.current?.focus());
  };

  const handleSave = (entry: CustomVocabEntry) => {
    if (editor?.index === null) {
      setSettings((current) => ({
        ...current,
        customVocabulary: [...current.customVocabulary, entry],
      }));
      setCurrentPage(
        Math.ceil(
          (settings.customVocabulary.length + 1) / VOCABULARY_PAGE_SIZE,
        ),
      );
    } else if (editor) {
      setSettings((current) => ({
        ...current,
        customVocabulary: current.customVocabulary.map((currentEntry, index) =>
          index === editor.index ? entry : currentEntry,
        ),
      }));
    }
    closeEditor();
  };

  const handleDelete = () => {
    if (editor?.index === null || !editor) return;
    setSettings((current) => ({
      ...current,
      customVocabulary: current.customVocabulary.filter(
        (_, index) => index !== editor.index,
      ),
    }));
    closeEditor();
  };

  const handleImport = async () => {
    const entries = await importVocabularyFromFile();
    if (!entries || entries.length === 0) return;
    const merged = mergeVocabulary(settings.customVocabulary, entries);
    setSettings((current) => ({
      ...current,
      customVocabulary: mergeVocabulary(current.customVocabulary, entries),
    }));
    setCurrentPage(
      Math.max(1, Math.ceil(merged.length / VOCABULARY_PAGE_SIZE)),
    );
  };

  return (
    <div className="setup-step setup-step-center setup-step--stretch vocab-page">
      <h2 className="setup-title">Custom Vocabulary</h2>
      <p className="setup-desc setup-desc--tight">
        Help Gladia recognize names, acronyms, and product terminology.
      </p>
      <div className="vocab-guidance">
        <span>Recommended intensity: 0.4–0.6</span>
        <button
          type="button"
          className="doc-link"
          onClick={() => void invoke("open_external_url", { url: DOCS_URL })}
        >
          Learn more ↗
        </button>
      </div>

      <div className="vocab-toolbar">
        <button
          type="button"
          className="btn btn-primary"
          onClick={(event) =>
            openEditor(
              {
                index: null,
                entry: { value: "", intensity: DEFAULT_VOCAB_INTENSITY },
              },
              event.currentTarget,
            )
          }
        >
          Add term
        </button>
        <div className="vocab-toolbar-secondary">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => void handleImport()}
          >
            Import CSV
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() =>
              void downloadVocabularyExport(settings.customVocabulary)
            }
            disabled={vocabularyLength === 0}
          >
            Export CSV
          </button>
        </div>
      </div>

      <div className="vocab-summary-list">
        <div className="vocab-summary-header" aria-hidden="true">
          <span>Term</span>
          <span>Intensity</span>
          <span>Pronunciations</span>
          <span />
        </div>
        {visibleEntries.length > 0 ? (
          visibleEntries.map((entry, visibleIndex) => {
            const index = pageStart + visibleIndex;
            const pronunciations = entry.pronunciations ?? [];
            const preview = pronunciations.slice(0, 2).join(", ");
            const remaining = Math.max(0, pronunciations.length - 2);
            return (
              <button
                type="button"
                className="vocab-summary-row"
                key={`${entry.value}-${index}`}
                onClick={(event) =>
                  openEditor(
                    {
                      index,
                      entry,
                    },
                    event.currentTarget,
                  )
                }
                aria-haspopup="dialog"
                aria-label={`Edit vocabulary term ${entry.value}`}
              >
                <span className="vocab-summary-term">{entry.value}</span>
                <span className="vocab-summary-intensity">
                  <strong>
                    {(entry.intensity ?? DEFAULT_VOCAB_INTENSITY).toFixed(2)}
                  </strong>
                </span>
                <span className="vocab-summary-pronunciations">
                  {pronunciations.length > 0 ? (
                    <>
                      <strong>
                        {pronunciations.length}{" "}
                        {pronunciations.length === 1 ? "variant" : "variants"}
                      </strong>
                      <small>
                        {preview}
                        {remaining > 0 ? ` +${remaining}` : ""}
                      </small>
                    </>
                  ) : (
                    <small>No variants</small>
                  )}
                </span>
                <span className="vocab-summary-edit" aria-hidden="true">
                  Edit
                </span>
              </button>
            );
          })
        ) : (
          <div className="vocab-summary-empty">
            <strong>No vocabulary terms yet</strong>
            <span>
              Add a term to improve recognition for words unique to you.
            </span>
          </div>
        )}
      </div>

      <div className="vocab-pagination" aria-live="polite">
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          disabled={currentPage <= 1}
          onClick={() => setCurrentPage((page) => page - 1)}
        >
          Previous
        </button>
        <span>
          Page {currentPage} of {totalPages} · {vocabularyLength}{" "}
          {vocabularyLength === 1 ? "term" : "terms"}
        </span>
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          disabled={currentPage >= totalPages}
          onClick={() => setCurrentPage((page) => page + 1)}
        >
          Next
        </button>
      </div>

      {editor && (
        <VocabularyEditorDialog
          entry={editor.entry}
          editingIndex={editor.index}
          entries={settings.customVocabulary}
          onSave={handleSave}
          onDelete={handleDelete}
          onClose={closeEditor}
        />
      )}
    </div>
  );
}
