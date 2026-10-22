import { renderToStaticMarkup } from "react-dom/server";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { groupMeetingUtterances, MeetingView } from "./MeetingView";
import { DEFAULT_MEETING_PREFERENCES } from "../lib/meetingWorkspace";

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
      meetings={[]}
      selectedMeeting={null}
      loading={false}
      calendar={null}
      calendarLoading={false}
      error={null}
      liveTitle=""
      liveNotes=""
      preferences={DEFAULT_MEETING_PREFERENCES}
      onPreferencesChange={vi.fn()}
      onLiveTitleChange={vi.fn()}
      onLiveNotesChange={vi.fn()}
      onStart={vi.fn()}
      onStartFromCalendar={vi.fn()}
      onConnectCalendar={vi.fn()}
      onOpenCalendarMeeting={vi.fn()}
      onStop={vi.fn()}
      onPause={vi.fn()}
      onResume={vi.fn()}
      onSelectMeeting={vi.fn()}
      onUpdateMeeting={vi.fn()}
      onDeleteMeeting={vi.fn()}
      onExportMarkdown={vi.fn()}
      onSaveMarkdown={vi.fn()}
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
    expect(render({ phase: "recording" })).toContain("Stop");
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

  it("keeps the completed transcript while a saved record is loading", () => {
    const html = render({
      utterances: [{ text: "Done", channel: 0, owner: true, isFinal: true }],
    });
    expect(html).toContain("Done");
    expect(html).toContain("Stop");
  });

  it("renders a searchable local library", () => {
    const html = render({
      meetings: [
        {
          id: "meeting-1",
          title: "Weekly sync",
          notes: "",
          summary: "",
          folder: "Team",
          duration: 120,
          speakerNames: {},
          text: "Launch update",
          utterances: [],
          createdAt: "2026-10-22T12:00:00Z",
          updatedAt: "2026-10-22T12:00:00Z",
        },
      ],
    });
    expect(html).toContain("Search meetings");
    expect(html).toContain("Weekly sync");
    expect(html).toContain("Team");
  });

  it("shows upcoming on-device Calendar meetings", () => {
    const html = render({
      calendar: {
        status: "authorized",
        meetings: [
          {
            id: "calendar-1",
            title: "Design review",
            startAt: "2026-10-22T14:00:00Z",
            endAt: "2026-10-22T15:00:00Z",
            joinUrl: "https://meet.example.com/design",
          },
        ],
      },
    });

    expect(html).toContain("Design review");
    expect(html).toContain(">Join</button>");
    expect(html).toContain(">Start</button>");
  });

  it("shows pause while recording and resume while paused", () => {
    expect(render({ phase: "recording" })).toContain("Pause");
    expect(render({ phase: "paused" })).toContain("Resume");
  });

  it("uses meeting preferences for live and saved views", () => {
    const liveHtml = render({
      phase: "recording",
      preferences: {
        ...DEFAULT_MEETING_PREFERENCES,
        showLiveTranscript: false,
      },
    });
    expect(liveHtml).not.toContain('role="tab">Transcript');
    expect(liveHtml).toContain('aria-label="My thoughts"');

    const savedHtml = render({
      meetings: [
        {
          id: "meeting-1",
          title: "Planning",
          notes: "A note",
          summary: "A local summary",
          folder: "Team",
          duration: 60,
          speakerNames: { "0": "Jordan" },
          text: "A transcript",
          utterances: [
            {
              text: "A transcript",
              channel: 1,
              owner: false,
              speaker: 0,
            },
          ],
          createdAt: "2026-10-22T12:00:00Z",
          updatedAt: "2026-10-22T12:00:00Z",
        },
      ],
      selectedMeeting: {
        id: "meeting-1",
        title: "Planning",
        notes: "A note",
        summary: "A local summary",
        folder: "Team",
        duration: 60,
        speakerNames: { "0": "Jordan" },
        text: "A transcript",
        utterances: [
          {
            text: "A transcript",
            channel: 1,
            owner: false,
            speaker: 0,
          },
        ],
        createdAt: "2026-10-22T12:00:00Z",
        updatedAt: "2026-10-22T12:00:00Z",
      },
      preferences: {
        ...DEFAULT_MEETING_PREFERENCES,
        defaultSavedMeetingTab: "summary",
      },
    });
    expect(savedHtml).toContain('aria-selected="true">Summary');
    expect(savedHtml).toContain("A local summary");
    expect(savedHtml).toContain("Save .md");
    expect(savedHtml).toContain('<option value="Jordan"></option>');
    expect(savedHtml).toContain('<option value="Team"></option>');
  });
});
