import { useEffect, useState } from "react";
import type {
  TranscriptionHistoryEntry,
  TranscriptionHistoryPage,
} from "../types";
import { TranscriptionHistory } from "./TranscriptionHistory";

export const HISTORY_PAGE_SIZE = 4;
const SEARCH_DEBOUNCE_MS = 300;

export function getHistoryPaginationSummary({
  isLoading,
  total,
  currentPage,
  pageSize,
}: {
  isLoading: boolean;
  total: number;
  currentPage: number;
  pageSize: number;
}): string {
  if (isLoading) return "Loading…";
  if (total === 0) return "Page 1 of 1 · 0 items";

  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const rangeStart = (currentPage - 1) * pageSize + 1;
  const rangeEnd = Math.min(currentPage * pageSize, total);
  return `Page ${currentPage} of ${totalPages} · Items ${rangeStart}–${rangeEnd} of ${total}`;
}

export function HistoryView({
  query,
  onQueryChange,
  page,
  onPageChange,
  pageData,
  isLoading,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  page: number;
  onPageChange: (page: number) => void;
  pageData: TranscriptionHistoryPage | null;
  isLoading: boolean;
}) {
  const [searchInput, setSearchInput] = useState(query);

  useEffect(() => {
    setSearchInput(query);
  }, [query]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      if (searchInput !== query) {
        onQueryChange(searchInput);
      }
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [searchInput, query, onQueryChange]);

  const total = pageData?.total ?? 0;
  const pageSize = pageData?.page_size ?? HISTORY_PAGE_SIZE;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = pageData?.page ?? page;
  const entries: TranscriptionHistoryEntry[] = pageData?.items ?? [];
  const paginationSummary = getHistoryPaginationSummary({
    isLoading,
    total,
    currentPage,
    pageSize,
  });

  return (
    <div className="setup-step history-page">
      <div className="history-header">
        <h2 className="setup-title">Transcription History</h2>
        <p className="setup-desc setup-desc--tight">
          Transcriptions from GladiaFlow on this device.
        </p>

        <div className="history-toolbar">
          <input
            type="search"
            className="form-input history-search-input"
            placeholder="Search transcriptions…"
            value={searchInput}
            onChange={(event) => setSearchInput(event.target.value)}
            aria-label="Search transcription history"
          />
        </div>
      </div>

      <TranscriptionHistory
        entries={entries}
        isLoading={isLoading}
        previewLength={200}
        flat
      />

      <div className="history-pagination" aria-live="polite">
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          disabled={isLoading || currentPage <= 1}
          onClick={() => onPageChange(currentPage - 1)}
        >
          Previous
        </button>
        <span className="history-pagination-summary">{paginationSummary}</span>
        <button
          type="button"
          className="btn btn-ghost btn-sm"
          disabled={isLoading || currentPage >= totalPages}
          onClick={() => onPageChange(currentPage + 1)}
        >
          Next
        </button>
      </div>
    </div>
  );
}
