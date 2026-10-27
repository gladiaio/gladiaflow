import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { TranscriptionHistoryEntry } from "../types";
import { TranscriptionHistory } from "./TranscriptionHistory";

const entry = (
  overrides: Partial<TranscriptionHistoryEntry>,
): TranscriptionHistoryEntry => ({
  id: "sess-1",
  text: "hello world",
  created_at: "2026-10-19T10:00:00Z",
  api_base_url: "https://api.gladia.io",
  open_url: "https://app.gladia.io/transcriptions/live/sess-1",
  ...overrides,
});

const render = (entries: TranscriptionHistoryEntry[]) =>
  renderToStaticMarkup(
    <TranscriptionHistory entries={entries} isLoading={false} />,
  );

describe("TranscriptionHistory", () => {
  it("enables Open when the session has a dashboard link", () => {
    const html = render([entry({})]);
    expect(html).toContain('aria-label="Open transcript in Gladia"');
    expect(html).not.toContain("disabled");
  });

  it("disables Open with a hint for a custom API URL", () => {
    const html = render([
      entry({ api_base_url: "http://localhost:8080", open_url: null }),
    ]);
    expect(html).toMatch(/<button[^>]*disabled/);
    expect(html).toContain(
      'aria-label="No Gladia dashboard link for this custom API URL (http://localhost:8080)"',
    );
    // The webview shows no native title tooltip: the hint is a focusable
    // Tooltip trigger instead.
    expect(html).not.toContain("title=");
    expect(html).toMatch(
      /<span class="transcription-history-open-disabled" tabindex="0"/,
    );
  });
});
