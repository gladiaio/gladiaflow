import { renderToStaticMarkup } from "react-dom/server";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { MeetingView } from "./MeetingView";

const render = (overrides: Partial<ComponentProps<typeof MeetingView>> = {}) =>
  renderToStaticMarkup(
    <MeetingView
      phase="idle"
      systemAudioCapability={{
        backend: "core_audio_process_tap",
        permission: "authorized",
        requiresRestart: false,
      }}
      utterances={[]}
      error={null}
      onStart={vi.fn()}
      onStop={vi.fn()}
      onGrantPermission={vi.fn()}
      onOpenSettings={vi.fn()}
      onRecheckPermission={vi.fn()}
      {...overrides}
    />,
  );

describe("MeetingView", () => {
  it("keeps the owner fixed and labels diarized remote speakers", () => {
    const html = render({
      utterances: [
        { text: "Owner words", channel: 0, owner: true },
        {
          text: "Remote words",
          channel: 1,
          owner: false,
          speaker: 1,
        },
        { text: "Unlabelled words", channel: 1, owner: false },
      ],
    });
    expect(html).toContain("<strong>You</strong>");
    expect(html).toContain("<strong>Speaker 2</strong>");
    expect(html).toContain("<strong>Others</strong>");
  });

  it("blocks start and offers permission actions without Screen Recording", () => {
    const html = render({
      systemAudioCapability: {
        backend: "screen_capture_kit",
        permission: "not_determined",
        requiresRestart: false,
      },
    });
    expect(html).toContain("Screen Recording permission is required");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Start meeting/);
    expect(html).toContain("Open System Settings");
  });

  it("names the modern system-audio permission", () => {
    const html = render({
      systemAudioCapability: {
        backend: "core_audio_process_tap",
        permission: "denied",
        requiresRestart: false,
      },
    });
    expect(html).toContain("System Audio Recording permission is required");
  });

  it("shows stop while recording", () => {
    expect(render({ phase: "recording" })).toContain("Stop meeting");
  });
});
