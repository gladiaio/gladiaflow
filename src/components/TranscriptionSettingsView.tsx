import { useEffect, useState } from "react";
import type { AppSettings } from "../types";
import { InfoTooltip } from "./InfoTooltip";
import { SettingsPageLayout } from "./SettingsPageLayout";

const LANGUAGE_PAGE_SIZE = 5;

export function TranscriptionSettingsView({
  settings,
  setSettings,
  languageDropdownOpen,
  setLanguageDropdownOpen,
  languageDropdownRef,
  handleOpenDropdown,
  handleLanguageToggle,
  languageSearch,
  setLanguageSearch,
  filteredLanguageOptions,
  selectedLanguageSummary,
  onDone,
}: {
  settings: AppSettings;
  setSettings: React.Dispatch<React.SetStateAction<AppSettings>>;
  languageDropdownOpen: boolean;
  setLanguageDropdownOpen: (v: boolean) => void;
  languageDropdownRef: React.RefObject<HTMLDivElement | null>;
  handleOpenDropdown: (d: "languages") => void;
  handleLanguageToggle: (code: string, checked: boolean) => void;
  languageSearch: string;
  setLanguageSearch: (v: string) => void;
  filteredLanguageOptions: readonly { code: string; label: string }[];
  selectedLanguageSummary: string;
  onDone: () => void;
}) {
  const endpointingValue = Number.isFinite(settings.endpointing)
    ? settings.endpointing
    : 0.1;
  const [languagePage, setLanguagePage] = useState(1);
  const totalLanguagePages = Math.max(
    1,
    Math.ceil(filteredLanguageOptions.length / LANGUAGE_PAGE_SIZE),
  );
  const visibleLanguageOptions = filteredLanguageOptions.slice(
    (languagePage - 1) * LANGUAGE_PAGE_SIZE,
    languagePage * LANGUAGE_PAGE_SIZE,
  );

  useEffect(() => {
    setLanguagePage(1);
  }, [languageSearch]);

  useEffect(() => {
    setLanguagePage((page) => Math.min(page, totalLanguagePages));
  }, [totalLanguagePages]);

  return (
    <SettingsPageLayout title="Transcription settings">
      <div className="settings-row">
        <div className="form-group">
          <label className="form-label">Languages</label>
          <div
            className={`multi-select ${languageDropdownOpen ? "open" : ""}`}
            ref={languageDropdownRef}
          >
            <button
              type="button"
              className="form-input multi-select-trigger"
              onClick={() => {
                if (languageDropdownOpen) {
                  setLanguageDropdownOpen(false);
                } else {
                  handleOpenDropdown("languages");
                }
              }}
              aria-haspopup="listbox"
              aria-expanded={languageDropdownOpen}
            >
              <span>{selectedLanguageSummary}</span>
            </button>
            {languageDropdownOpen && (
              <div className="multi-select-dropdown">
                <input
                  type="text"
                  value={languageSearch}
                  onChange={(e) => setLanguageSearch(e.target.value)}
                  className="form-input multi-select-search"
                  placeholder="Search languages..."
                  autoCorrect="off"
                  autoCapitalize="none"
                  autoComplete="off"
                  spellCheck={false}
                />
                <div
                  className="multi-select-options"
                  role="listbox"
                  aria-multiselectable="true"
                >
                  {filteredLanguageOptions.length > 0 ? (
                    visibleLanguageOptions.map((language) => (
                      <label
                        key={language.code}
                        className="form-checkbox multi-select-option"
                      >
                        <input
                          type="checkbox"
                          checked={settings.languages.includes(language.code)}
                          onChange={(e) =>
                            handleLanguageToggle(
                              language.code,
                              e.target.checked,
                            )
                          }
                        />
                        {language.label}
                      </label>
                    ))
                  ) : (
                    <div className="multi-select-empty">
                      No matching languages
                    </div>
                  )}
                </div>
                {filteredLanguageOptions.length > LANGUAGE_PAGE_SIZE && (
                  <div className="language-pagination" aria-live="polite">
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      disabled={languagePage <= 1}
                      onClick={() => setLanguagePage((page) => page - 1)}
                    >
                      Previous
                    </button>
                    <span>
                      {languagePage} / {totalLanguagePages}
                    </span>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      disabled={languagePage >= totalLanguagePages}
                      onClick={() => setLanguagePage((page) => page + 1)}
                    >
                      Next
                    </button>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
        <div className="form-group">
          <label className="form-label">Code switching</label>
          <label className="toggle-switch" title="Code switching">
            <input
              type="checkbox"
              checked={settings.codeSwitching}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  codeSwitching: e.target.checked,
                })
              }
            />
            <span className="toggle-slider" />
          </label>
        </div>
      </div>

      <div className="form-group">
        <label className="form-label">
          Endpointing
          <InfoTooltip label="About endpointing">
            <strong>Endpointing</strong>
            How long (in seconds) Gladia waits for silence before treating an
            utterance as finished. Lower values feel snappier but may cut
            sentences short; higher values wait longer so pauses don&apos;t
            split your speech.
          </InfoTooltip>
        </label>
        <div className="endpointing-control">
          <input
            type="range"
            className="endpointing-slider"
            min={0.05}
            max={1}
            step={0.05}
            value={endpointingValue}
            onChange={(e) =>
              setSettings({
                ...settings,
                endpointing: parseFloat(e.target.value),
              })
            }
          />
          <span className="endpointing-value">
            {endpointingValue.toFixed(2)}s
          </span>
        </div>
      </div>

      <div className="setup-nav setup-nav-center">
        <button className="btn btn-primary" onClick={onDone}>
          Done
        </button>
      </div>
    </SettingsPageLayout>
  );
}
