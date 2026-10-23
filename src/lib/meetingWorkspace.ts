import type {
  CalendarMeeting,
  MeetingRecord,
  MeetingUtterance,
} from "../types";

export type MeetingTab = "thoughts" | "transcript" | "summary";
export type MeetingMaximumMinutes = 0 | 30 | 60 | 120;

export interface MeetingPreferences {
  maximumMinutes: MeetingMaximumMinutes;
  showLiveTranscript: boolean;
  automaticallyCreateSummary: boolean;
  defaultSavedMeetingTab: MeetingTab;
}

export const MEETING_PREFERENCES_STORAGE_KEY =
  "gladiaflow.meetingPreferences.v1";

export const DEFAULT_MEETING_PREFERENCES: MeetingPreferences = {
  maximumMinutes: 0,
  showLiveTranscript: true,
  automaticallyCreateSummary: true,
  defaultSavedMeetingTab: "transcript",
};

const MAXIMUM_MINUTE_OPTIONS = new Set<MeetingMaximumMinutes>([0, 30, 60, 120]);
const MEETING_TAB_OPTIONS = new Set<MeetingTab>([
  "thoughts",
  "transcript",
  "summary",
]);

export function parseMeetingPreferences(
  value: string | null,
): MeetingPreferences {
  if (!value) return DEFAULT_MEETING_PREFERENCES;
  try {
    const parsed = JSON.parse(value) as Partial<MeetingPreferences>;
    return {
      maximumMinutes: MAXIMUM_MINUTE_OPTIONS.has(
        parsed.maximumMinutes as MeetingMaximumMinutes,
      )
        ? (parsed.maximumMinutes as MeetingMaximumMinutes)
        : DEFAULT_MEETING_PREFERENCES.maximumMinutes,
      showLiveTranscript:
        typeof parsed.showLiveTranscript === "boolean"
          ? parsed.showLiveTranscript
          : DEFAULT_MEETING_PREFERENCES.showLiveTranscript,
      automaticallyCreateSummary:
        typeof parsed.automaticallyCreateSummary === "boolean"
          ? parsed.automaticallyCreateSummary
          : DEFAULT_MEETING_PREFERENCES.automaticallyCreateSummary,
      defaultSavedMeetingTab: MEETING_TAB_OPTIONS.has(
        parsed.defaultSavedMeetingTab as MeetingTab,
      )
        ? (parsed.defaultSavedMeetingTab as MeetingTab)
        : DEFAULT_MEETING_PREFERENCES.defaultSavedMeetingTab,
    };
  } catch {
    return DEFAULT_MEETING_PREFERENCES;
  }
}

export function loadMeetingPreferences(
  storage: Pick<Storage, "getItem"> | undefined = typeof localStorage ===
  "undefined"
    ? undefined
    : localStorage,
): MeetingPreferences {
  if (!storage) return DEFAULT_MEETING_PREFERENCES;
  try {
    return parseMeetingPreferences(
      storage.getItem(MEETING_PREFERENCES_STORAGE_KEY),
    );
  } catch {
    return DEFAULT_MEETING_PREFERENCES;
  }
}

export function meetingSuggestions(meetings: MeetingRecord[]): {
  folders: string[];
  speakerNames: string[];
} {
  const unique = (values: string[]) =>
    [...new Set(values.map((value) => value.trim()).filter(Boolean))].sort(
      (left, right) => left.localeCompare(right),
    );
  return {
    folders: unique(meetings.map(({ folder }) => folder)),
    speakerNames: unique(
      meetings.flatMap(({ speakerNames }) => Object.values(speakerNames)),
    ),
  };
}

export function remainingMeetingDurationMs(
  maximumMinutes: MeetingMaximumMinutes,
  recordedMs: number,
): number | null {
  if (maximumMinutes === 0) return null;
  return Math.max(0, maximumMinutes * 60_000 - Math.max(0, recordedMs));
}

const STOP_WORDS = new Set([
  "a",
  "an",
  "and",
  "are",
  "as",
  "at",
  "be",
  "but",
  "by",
  "for",
  "from",
  "has",
  "have",
  "he",
  "i",
  "in",
  "is",
  "it",
  "of",
  "on",
  "or",
  "our",
  "she",
  "so",
  "that",
  "the",
  "their",
  "they",
  "this",
  "to",
  "was",
  "we",
  "were",
  "will",
  "with",
  "you",
]);

const words = (text: string) =>
  (text.toLowerCase().match(/[\p{L}\p{N}']+/gu) ?? []).filter(
    (word) => word.length > 1 && !STOP_WORDS.has(word),
  );

const sentences = (text: string) =>
  (text.match(/[^.!?\n]+(?:[.!?]+|$)/g) ?? [])
    .map((sentence) => sentence.trim())
    .filter(Boolean);

export function generateMeetingSummary(text: string, limit = 4): string {
  const candidates = sentences(text);
  if (candidates.length <= limit) return candidates.join(" ");

  const frequencies = new Map<string, number>();
  words(text).forEach((word) =>
    frequencies.set(word, (frequencies.get(word) ?? 0) + 1),
  );
  const selected = candidates
    .map((sentence, index) => {
      const terms = words(sentence);
      const score =
        terms.reduce((total, term) => total + (frequencies.get(term) ?? 0), 0) /
          Math.max(terms.length, 1) +
        (index === 0 ? 0.5 : 0);
      return { sentence, index, score };
    })
    .sort((left, right) => right.score - left.score || left.index - right.index)
    .slice(0, limit)
    .sort((left, right) => left.index - right.index);

  return selected.map(({ sentence }) => sentence).join(" ");
}

export interface TranscriptMatch {
  text: string;
  speaker: string;
  score: number;
}

const speakerName = (
  utterance: MeetingUtterance,
  names: Record<string, string>,
) => {
  if (utterance.owner) return "You";
  const key = String(utterance.speaker ?? 0);
  return names[key] || `Speaker ${Number(key) + 1}`;
};

export function searchTranscript(
  query: string,
  utterances: MeetingUtterance[],
  speakerNames: Record<string, string> = {},
  limit = 5,
): TranscriptMatch[] {
  const queryTerms = new Set(words(query));
  if (queryTerms.size === 0) return [];

  return utterances
    .map((utterance, index) => {
      const textTerms = words(utterance.text);
      const overlap = textTerms.filter((term) => queryTerms.has(term)).length;
      const phraseBonus = utterance.text
        .toLowerCase()
        .includes(query.trim().toLowerCase())
        ? 2
        : 0;
      return {
        text: utterance.text.trim(),
        speaker: speakerName(utterance, speakerNames),
        score: overlap + phraseBonus,
        index,
      };
    })
    .filter(({ text, score }) => text && score > 0)
    .sort((left, right) => right.score - left.score || left.index - right.index)
    .slice(0, limit)
    .map(({ text, speaker, score }) => ({ text, speaker, score }));
}

export const meetingDisplayTitle = (meeting: MeetingRecord) =>
  meeting.title.trim() || "Untitled meeting";

export function currentCalendarMeeting(
  meetings: CalendarMeeting[],
  now = Date.now(),
  leadMinutes = 5,
): CalendarMeeting | null {
  const leadMs = Math.max(0, leadMinutes) * 60_000;
  return (
    meetings.find((meeting) => {
      const start = Date.parse(meeting.startAt);
      const end = Date.parse(meeting.endAt);
      return (
        Number.isFinite(start) &&
        Number.isFinite(end) &&
        start <= now + leadMs &&
        end > now
      );
    }) ?? null
  );
}

export function mergeMeetingUtterance(
  current: MeetingUtterance[],
  incoming: MeetingUtterance,
): MeetingUtterance[] {
  const belongsToIncoming = (utterance: MeetingUtterance) => {
    if (utterance.channel !== incoming.channel) return false;
    if (utterance.id && incoming.id && utterance.id === incoming.id) return true;

    const start = utterance.start;
    const end = utterance.end;
    const incomingStart = incoming.start;
    const incomingEnd = incoming.end;
    if (
      start === undefined ||
      end === undefined ||
      incomingStart === undefined ||
      incomingEnd === undefined ||
      start < 0 ||
      end < 0 ||
      incomingStart < 0 ||
      incomingEnd < 0
    ) {
      return !utterance.id || !incoming.id;
    }
    return Math.max(start, incomingStart) <= Math.min(end, incomingEnd);
  };

  const withoutOpenPartial = current.filter(
    (utterance) =>
      utterance.isFinal !== false || !belongsToIncoming(utterance),
  );
  const withoutDuplicateFinal =
    incoming.isFinal === false
      ? withoutOpenPartial
      : withoutOpenPartial.filter(
          (utterance) =>
            utterance.isFinal === false || !belongsToIncoming(utterance),
        );

  return [...withoutDuplicateFinal, incoming].sort((left, right) => {
    const startDifference = (left.start ?? 0) - (right.start ?? 0);
    if (startDifference !== 0) return startDifference;
    if (left.isFinal === right.isFinal) return 0;
    return left.isFinal === false ? 1 : -1;
  });
}
