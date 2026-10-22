import { renderToStaticMarkup } from "react-dom/server";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { groupMeetingUtterances, MeetingView } from "./MeetingView";

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
    expect(html).toMatch(
      /<button[^>]*meeting-start-button[^>]*disabled=""[^>]*>/,
    );
    expect(html).toContain("Start meeting");
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

  it("keeps finalized text visible while showing the current partial", () => {
    const html = render({
      phase: "recording",
      utterances: [
        {
          id: "final",
          text: "Finalized sentence",
          channel: 1,
          owner: false,
          speaker: 0,
          isFinal: true,
        },
        {
          id: "partial",
          text: "Current partial",
          channel: 1,
          owner: false,
          speaker: 1,
          isFinal: false,
        },
      ],
    });

    expect(html).toContain("Finalized sentence");
    expect(html).toContain("Current partial");
    expect(html).toContain("Listening…");
    expect(html).toContain("2 speakers");
  });

  it("groups consecutive phrases from the same speaker into one turn", () => {
    const turns = groupMeetingUtterances([
      { id: "1", text: "One", channel: 1, owner: false, speaker: 0 },
      { id: "2", text: "Two", channel: 1, owner: false, speaker: 0 },
      { id: "3", text: "Three", channel: 0, owner: true },
    ]);

    expect(turns).toHaveLength(2);
    expect(turns[0].label).toBe("Speaker 1");
    expect(turns[0].utterances.map(({ text }) => text)).toEqual(["One", "Two"]);
    expect(turns[1].label).toBe("You");
  });

  it("keeps the completed transcript and offers a new meeting", () => {
    const html = render({
      utterances: [{ text: "Done", channel: 0, owner: true, isFinal: true }],
    });
    expect(html).toContain("Done");
    expect(html).toContain("Saved");
    expect(html).toContain("New meeting");
  });
});
