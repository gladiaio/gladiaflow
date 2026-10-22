import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { MeetingCalendarPrompt } from "./MeetingCalendarPrompt";

describe("MeetingCalendarPrompt", () => {
  it("offers a joined start for a current Calendar meeting", () => {
    const html = renderToStaticMarkup(
      <MeetingCalendarPrompt
        meeting={{
          id: "retro",
          title: "Retro",
          startAt: "2026-10-22T14:00:00Z",
          endAt: "2026-10-22T15:00:00Z",
          joinUrl: "https://meet.example.com/retro",
        }}
        onJoinAndStart={vi.fn()}
        onStart={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );

    expect(html).toContain("Retro");
    expect(html).toContain("Join meeting");
    expect(html).toContain("&amp; start Meeting");
    expect(html).toContain('aria-label="Dismiss meeting reminder"');
  });
});
