import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranscriptionHistoryEntry } from "../types";

function formatTimestamp(createdAt: string): string {
  if (!createdAt) return "Unknown time";
  const date = new Date(createdAt);
  if (Number.isNaN(date.getTime())) return createdAt;
  return date.toLocaleString();
}

function truncateText(text: string, maxLength: number): string {
  const trimmed = text.trim();
  if (trimmed.length <= maxLength) return trimmed;
  return `${trimmed.slice(0, maxLength).trimEnd()}…`;
}

function liveTranscriptionUrl(sessionId: string): string {
  return `https://app.gladia.io/transcriptions/live/${sessionId}`;
}

export function TranscriptionHistory({
  entries,
  isLoading,
  previewLength = 80,
  flat = false,
}: {
  entries: TranscriptionHistoryEntry[];
  isLoading: boolean;
  previewLength?: number;
  flat?: boolean;
}) {
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const handleCopy = async (entry: TranscriptionHistoryEntry) => {
    try {
      await navigator.clipboard.writeText(entry.text);
      setCopiedId(entry.id);
      window.setTimeout(() => {
        setCopiedId((current) => (current === entry.id ? null : current));
      }, 1500);
    } catch (error) {
      console.error("Failed to copy transcript:", error);
    }
  };

  const handleOpen = async (
    event: React.MouseEvent,
    entry: TranscriptionHistoryEntry,
  ) => {
    event.stopPropagation();
    try {
      await invoke("open_external_url", {
        url: liveTranscriptionUrl(entry.id),
      });
    } catch (error) {
      console.error("Failed to open transcript in browser:", error);
    }
  };

  const handleRowKeyDown = (
    event: React.KeyboardEvent,
    entry: TranscriptionHistoryEntry,
  ) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      void handleCopy(entry);
    }
  };

  const body = isLoading ? (
    <ul
      className="transcription-history-list transcription-history-list--loading"
      aria-label="Loading transcriptions"
    >
      {Array.from({ length: 4 }, (_, index) => (
        <li
          key={index}
          className="transcription-history-item transcription-history-item--skeleton"
          aria-hidden="true"
        >
          <div className="transcription-history-skeleton-lines">
            <span />
            <span />
          </div>
        </li>
      ))}
    </ul>
  ) : entries.length === 0 ? (
    <p className="form-footnote">No transcriptions yet.</p>
  ) : (
    <ul className="transcription-history-list">
      {entries.map((entry) => {
        const isCopied = copiedId === entry.id;
        return (
          <li
            key={entry.id}
            className={`transcription-history-item${isCopied ? " transcription-history-item--copied" : ""}`}
            role="button"
            tabIndex={0}
            onClick={() => void handleCopy(entry)}
            onKeyDown={(event) => handleRowKeyDown(event, entry)}
            aria-label={`Copy transcription from ${formatTimestamp(entry.created_at)}`}
          >
            <div className="transcription-history-content">
              <p className="transcription-history-preview">
                {truncateText(entry.text, previewLength)}
              </p>
              <span className="transcription-history-time">
                {isCopied ? "Copied" : formatTimestamp(entry.created_at)}
              </span>
            </div>
            <div className="transcription-history-actions">
              <button
                type="button"
                className="btn btn-ghost btn-sm transcription-history-open"
                onClick={(event) => void handleOpen(event, entry)}
                aria-label="Open transcript in Gladia"
              >
                Open
              </button>
            </div>
          </li>
        );
      })}
    </ul>
  );

  if (flat) {
    return <div className="history-page-list">{body}</div>;
  }

  return (
    <div className="dictation-stats-card transcription-history-card">
      <h3 className="dictation-stats-title">Recent transcriptions</h3>
      {body}
    </div>
  );
}
