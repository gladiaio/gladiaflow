import { describe, expect, it } from "vitest";
import {
  DEFAULT_MEETING_PREFERENCES,
  currentCalendarMeeting,
  generateMeetingSummary,
  loadMeetingPreferences,
  mergeMeetingUtterance,
  meetingSuggestions,
  parseMeetingPreferences,
  remainingMeetingDurationMs,
  searchTranscript,
} from "./meetingWorkspace";

describe("meeting preferences", () => {
  it("loads valid saved values and fills missing values from defaults", () => {
    expect(
      parseMeetingPreferences(
        JSON.stringify({
          maximumMinutes: 60,
          showLiveTranscript: false,
          defaultSavedMeetingTab: "summary",
        }),
      ),
    ).toEqual({
      maximumMinutes: 60,
      showLiveTranscript: false,
      automaticallyCreateSummary: true,
      defaultSavedMeetingTab: "summary",
    });
  });

  it("falls back safely for invalid JSON, values, and inaccessible storage", () => {
    expect(parseMeetingPreferences("{")).toEqual(DEFAULT_MEETING_PREFERENCES);
    expect(
      parseMeetingPreferences(
        JSON.stringify({
          maximumMinutes: 45,
          defaultSavedMeetingTab: "unknown",
        }),
      ),
    ).toEqual(DEFAULT_MEETING_PREFERENCES);
    expect(
      loadMeetingPreferences({
        getItem: () => {
          throw new Error("blocked");
        },
      }),
    ).toEqual(DEFAULT_MEETING_PREFERENCES);
  });

  it("calculates the remaining active recording time", () => {
    expect(remainingMeetingDurationMs(0, 1_000)).toBeNull();
    expect(remainingMeetingDurationMs(30, 5 * 60_000)).toBe(25 * 60_000);
    expect(remainingMeetingDurationMs(30, 31 * 60_000)).toBe(0);
  });
});

describe("currentCalendarMeeting", () => {
  it("selects an ongoing or imminent local Calendar event", () => {
    const now = Date.parse("2026-10-22T14:00:00Z");
    const meeting = currentCalendarMeeting(
      [
        {
          id: "later",
          title: "Later",
          startAt: "2026-10-22T15:00:00Z",
          endAt: "2026-10-22T16:00:00Z",
        },
        {
          id: "retro",
          title: "Retro",
          startAt: "2026-10-22T14:04:00Z",
          endAt: "2026-10-22T14:30:00Z",
        },
      ],
      now,
    );
    expect(meeting?.id).toBe("retro");
  });

  it("ignores finished and distant events", () => {
    const now = Date.parse("2026-10-22T14:00:00Z");
    expect(
      currentCalendarMeeting(
        [
          {
            id: "finished",
            title: "Finished",
            startAt: "2026-10-22T13:00:00Z",
            endAt: "2026-10-22T13:30:00Z",
          },
        ],
        now,
      ),
    ).toBeNull();
  });
});

describe("mergeMeetingUtterance", () => {
  it("keeps one live partial per channel even when Gladia changes its id", () => {
    const current = [
      {
        id: "old",
        text: "old partial",
        channel: 1 as const,
        owner: false,
        isFinal: false,
        start: 1,
        end: 2,
      },
    ];
    expect(
      mergeMeetingUtterance(current, {
        id: "new",
        text: "new partial",
        channel: 1,
        owner: false,
        isFinal: false,
        start: 1,
        end: 3,
      }),
    ).toEqual([expect.objectContaining({ id: "new", text: "new partial" })]);
  });

  it("removes the open partial when its channel receives a final", () => {
    const merged = mergeMeetingUtterance(
      [
        {
          id: "partial-id",
          text: "provisional",
          channel: 1,
          owner: false,
          isFinal: false,
          start: 4,
          end: 5,
        },
        {
          id: "owner-partial",
          text: "owner speaking",
          channel: 0,
          owner: true,
          isFinal: false,
          start: 5,
          end: 6,
        },
      ],
      {
        id: "different-final-id",
        text: "confirmed",
        channel: 1,
        owner: false,
        isFinal: true,
        start: 4,
        end: 5,
      },
    );

    expect(merged).toEqual([
      expect.objectContaining({ text: "confirmed", isFinal: true }),
      expect.objectContaining({ text: "owner speaking", isFinal: false }),
    ]);
  });
});

describe("meetingSuggestions", () => {
  it("derives sorted, unique speaker and folder suggestions from meetings", () => {
    const meeting = {
      id: "1",
      title: "",
      notes: "",
      summary: "",
      folder: " Team ",
      duration: 1,
      speakerNames: { owner: "Jordan", "0": "Alex" },
      text: "",
      utterances: [],
      createdAt: "",
      updatedAt: "",
    };
    expect(
      meetingSuggestions([
        meeting,
        {
          ...meeting,
          id: "2",
          folder: "Clients",
          speakerNames: { "0": "Alex", "1": "Sam" },
        },
      ]),
    ).toEqual({
      folders: ["Clients", "Team"],
      speakerNames: ["Alex", "Jordan", "Sam"],
    });
  });
});

describe("generateMeetingSummary", () => {
  it("is deterministic and keeps selected sentences in transcript order", () => {
    const transcript =
      "We reviewed the launch plan. The launch needs approval. Jordan owns the launch checklist. Lunch is at noon. Approval is due Friday.";
    const first = generateMeetingSummary(transcript, 3);
    expect(generateMeetingSummary(transcript, 3)).toBe(first);
    expect(first).toContain("launch");
    expect(first.indexOf("reviewed")).toBeLessThan(first.indexOf("owns"));
  });

  it("returns short transcripts without inventing text", () => {
    expect(generateMeetingSummary("One decision. One action.")).toBe(
      "One decision. One action.",
    );
  });
});

describe("searchTranscript", () => {
  const utterances = [
    {
      text: "The launch checklist is ready for Friday.",
      channel: 1 as const,
      owner: false,
      speaker: 0,
    },
    {
      text: "I will review the budget tomorrow.",
      channel: 0 as const,
      owner: true,
    },
  ];

  it("returns direct relevant excerpts with saved speaker names", () => {
    expect(
      searchTranscript("launch Friday", utterances, { "0": "Jordan" }),
    ).toEqual([
      {
        text: "The launch checklist is ready for Friday.",
        speaker: "Jordan",
        score: 2,
      },
    ]);
  });

  it("returns no excerpts for unrelated or empty questions", () => {
    expect(searchTranscript("vacation", utterances)).toEqual([]);
    expect(searchTranscript("", utterances)).toEqual([]);
  });
});
