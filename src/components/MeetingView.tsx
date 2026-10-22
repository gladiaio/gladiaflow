import { useEffect, useMemo, useRef, useState } from "react";
import type {
  CalendarSnapshot,
  MeetingPhase,
  MeetingRecord,
  MeetingUpdate,
  MeetingUtterance,
  SystemAudioCapability,
} from "../types";
import {
  type MeetingPreferences,
  type MeetingTab,
  generateMeetingSummary,
  meetingSuggestions,
  meetingDisplayTitle,
  searchTranscript,
} from "../lib/meetingWorkspace";

interface MeetingTurn {
  key: string;
  speakerKey: string;
  label: string;
  owner: boolean;
  utterances: MeetingUtterance[];
}

const speakerLabel = (
  utterance: MeetingUtterance,
  names: Record<string, string> = {},
) => {
  if (utterance.owner) return "You";
  if (utterance.speaker === undefined) return "Others";
  const key = String(utterance.speaker);
  return names[key] || `Speaker ${Number(key) + 1}`;
};

export const groupMeetingUtterances = (
  utterances: MeetingUtterance[],
  names: Record<string, string> = {},
): MeetingTurn[] =>
  utterances.reduce<MeetingTurn[]>((turns, utterance, index) => {
    const speakerKey = utterance.owner
      ? "owner"
      : utterance.speaker === undefined
        ? "others"
        : String(utterance.speaker);
    const label = speakerLabel(utterance, names);
    const previous = turns[turns.length - 1];
    if (previous?.speakerKey === speakerKey) {
      previous.utterances.push(utterance);
      return turns;
    }
    turns.push({
      key: `${speakerKey}:${utterance.id ?? index}`,
      speakerKey,
      label,
      owner: utterance.owner,
      utterances: [utterance],
    });
    return turns;
  }, []);

const formatDuration = (seconds: number) => {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const tail = `${minutes.toString().padStart(2, "0")}:${(total % 60)
    .toString()
    .padStart(2, "0")}`;
  return hours ? `${hours.toString().padStart(2, "0")}:${tail}` : tail;
};

const formatDate = (value: string) => {
  const date = new Date(value);
  return Number.isNaN(date.valueOf())
    ? value
    : new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(date);
};

const formatTimeRange = (startValue: string, endValue: string) => {
  const start = new Date(startValue);
  const end = new Date(endValue);
  if (Number.isNaN(start.valueOf()) || Number.isNaN(end.valueOf())) return "";
  const formatter = new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
  return `${formatter.format(start)} – ${formatter.format(end)}`;
};

export interface MeetingViewProps {
  phase: MeetingPhase;
  systemAudioCapability: SystemAudioCapability | null;
  utterances: MeetingUtterance[];
  meetings: MeetingRecord[];
  selectedMeeting: MeetingRecord | null;
  loading: boolean;
  calendar: CalendarSnapshot | null;
  calendarLoading: boolean;
  error: string | null;
  liveTitle: string;
  liveNotes: string;
  preferences: MeetingPreferences;
  onPreferencesChange: (preferences: MeetingPreferences) => void;
  onLiveTitleChange: (value: string) => void;
  onLiveNotesChange: (value: string) => void;
  onStart: () => void;
  onStartFromCalendar: (title: string) => void;
  onConnectCalendar: () => void;
  onOpenCalendarMeeting: (url: string) => void;
  onStop: () => void;
  onPause: () => void;
  onResume: () => void;
  onSelectMeeting: (id: string | null) => void;
  onUpdateMeeting: (
    id: string,
    update: MeetingUpdate,
  ) => Promise<MeetingRecord>;
  onDeleteMeeting: (id: string) => Promise<void>;
  onExportMarkdown: (id: string) => Promise<string>;
  onSaveMarkdown: (id: string) => Promise<boolean>;
  onGrantPermission: () => void;
  onOpenSettings: () => void;
  onRecheckPermission: () => void;
}

export function MeetingView({
  phase,
  systemAudioCapability,
  utterances,
  meetings,
  selectedMeeting,
  loading,
  calendar,
  calendarLoading,
  error,
  liveTitle,
  liveNotes,
  preferences,
  onPreferencesChange,
  onLiveTitleChange,
  onLiveNotesChange,
  onStart,
  onStartFromCalendar,
  onConnectCalendar,
  onOpenCalendarMeeting,
  onStop,
  onPause,
  onResume,
  onSelectMeeting,
  onUpdateMeeting,
  onDeleteMeeting,
  onExportMarkdown,
  onSaveMarkdown,
  onGrantPermission,
  onOpenSettings,
  onRecheckPermission,
}: MeetingViewProps) {
  const active =
    phase !== "idle" || (utterances.length > 0 && !selectedMeeting);
  const permissionReady =
    systemAudioCapability?.permission === "authorized" ||
    (systemAudioCapability?.backend === "core_audio_process_tap" &&
      systemAudioCapability.permission === "not_determined");
  const permissionName =
    systemAudioCapability?.backend === "screen_capture_kit"
      ? "Screen Recording"
      : "System Audio Recording";
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<MeetingTab>(() =>
    selectedMeeting
      ? preferences.defaultSavedMeetingTab
      : preferences.showLiveTranscript
        ? "transcript"
        : "thoughts",
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [followingLive, setFollowingLive] = useState(true);
  const [title, setTitle] = useState("");
  const [notes, setNotes] = useState("");
  const [folder, setFolder] = useState("");
  const [transcriptSearch, setTranscriptSearch] = useState("");
  const [question, setQuestion] = useState("");
  const [askedQuestion, setAskedQuestion] = useState("");
  const [copied, setCopied] = useState(false);
  const [exported, setExported] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState(false);
  const startedAtRef = useRef<number | null>(null);
  const elapsedBaseRef = useRef(0);
  const previousPhaseRef = useRef<MeetingPhase>("idle");
  const transcriptRef = useRef<HTMLDivElement>(null);
  const notesInitializedRef = useRef(false);

  useEffect(() => {
    if (!selectedMeeting) return;
    setTitle(selectedMeeting.title);
    setNotes(selectedMeeting.notes);
    setFolder(selectedMeeting.folder);
    setTab(preferences.defaultSavedMeetingTab);
    setTranscriptSearch("");
    setAskedQuestion("");
    setDeleteConfirm(false);
    notesInitializedRef.current = false;
    queueMicrotask(() => {
      notesInitializedRef.current = true;
    });
  }, [selectedMeeting?.id, preferences.defaultSavedMeetingTab]);

  useEffect(() => {
    if (active && !selectedMeeting && !preferences.showLiveTranscript) {
      setTab((current) => (current === "transcript" ? "thoughts" : current));
    }
  }, [active, preferences.showLiveTranscript, selectedMeeting]);

  useEffect(() => {
    const selectedMeetingId = selectedMeeting?.id;
    if (!selectedMeetingId || !notesInitializedRef.current) return;
    const timer = window.setTimeout(() => {
      void onUpdateMeeting(selectedMeetingId, { notes });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [notes, onUpdateMeeting, selectedMeeting?.id]);

  useEffect(() => {
    const previousPhase = previousPhaseRef.current;
    previousPhaseRef.current = phase;
    if (phase === "starting") {
      if (previousPhase === "idle") {
        startedAtRef.current = null;
        elapsedBaseRef.current = 0;
        setElapsedSeconds(0);
      }
      setFollowingLive(true);
    }
    if (phase === "paused" && startedAtRef.current !== null) {
      elapsedBaseRef.current += Math.max(
        0,
        Math.floor((Date.now() - startedAtRef.current) / 1000),
      );
      startedAtRef.current = null;
      setElapsedSeconds(elapsedBaseRef.current);
    }
    if (phase === "recording" && startedAtRef.current === null) {
      startedAtRef.current = Date.now();
    }
    if (phase !== "recording") return;
    const update = () => {
      if (startedAtRef.current !== null) {
        setElapsedSeconds(
          elapsedBaseRef.current +
            Math.max(0, Math.floor((Date.now() - startedAtRef.current) / 1000)),
        );
      }
    };
    update();
    const interval = window.setInterval(update, 1000);
    return () => window.clearInterval(interval);
  }, [phase]);

  useEffect(() => {
    if (!followingLive || tab !== "transcript") return;
    const transcript = transcriptRef.current;
    if (transcript) transcript.scrollTop = transcript.scrollHeight;
  }, [followingLive, tab, utterances]);

  const visibleMeetings = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return meetings;
    return meetings.filter((meeting) =>
      [
        meeting.title,
        meeting.text,
        meeting.notes,
        meeting.folder,
        ...Object.values(meeting.speakerNames),
      ].some((value) => value.toLowerCase().includes(normalized)),
    );
  }, [meetings, query]);
  const suggestions = useMemo(() => meetingSuggestions(meetings), [meetings]);

  const currentUtterances = selectedMeeting?.utterances ?? utterances;
  const speakerNames = selectedMeeting?.speakerNames ?? {};
  const turns = useMemo(() => {
    if (selectedMeeting) {
      return groupMeetingUtterances(currentUtterances, speakerNames);
    }
    const finalized = currentUtterances.filter(
      (utterance) => utterance.isFinal !== false,
    );
    const partials = currentUtterances.filter(
      (utterance) => utterance.isFinal === false,
    );
    return [
      ...groupMeetingUtterances(finalized, speakerNames),
      ...groupMeetingUtterances(partials, speakerNames),
    ];
  }, [currentUtterances, selectedMeeting, speakerNames]);
  const speakerCount = new Set(turns.map((turn) => turn.speakerKey)).size;
  const finalText = currentUtterances
    .filter((item) => item.isFinal !== false)
    .map((item) => item.text)
    .join(" ");
  const localSummary = generateMeetingSummary(
    selectedMeeting?.text || finalText,
  );
  const transcriptMatches = searchTranscript(
    transcriptSearch,
    currentUtterances,
    speakerNames,
  );
  const answerMatches = searchTranscript(
    askedQuestion,
    currentUtterances,
    speakerNames,
    3,
  );

  useEffect(() => {
    if (
      !preferences.automaticallyCreateSummary ||
      !selectedMeeting ||
      selectedMeeting.summary ||
      !localSummary
    )
      return;
    void onUpdateMeeting(selectedMeeting.id, { summary: localSummary });
  }, [
    localSummary,
    onUpdateMeeting,
    preferences.automaticallyCreateSummary,
    selectedMeeting?.id,
    selectedMeeting?.summary,
  ]);

  const saveField = (update: MeetingUpdate) => {
    if (selectedMeeting) void onUpdateMeeting(selectedMeeting.id, update);
  };

  const copyMarkdown = async () => {
    if (!selectedMeeting) return;
    const markdown = await onExportMarkdown(selectedMeeting.id);
    await navigator.clipboard.writeText(markdown);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const copyTranscript = async () => {
    const transcript = turns
      .flatMap((turn) =>
        turn.utterances.map((item) => `${turn.label}: ${item.text.trim()}`),
      )
      .join("\n");
    if (!transcript) return;
    await navigator.clipboard.writeText(transcript);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const saveMarkdown = async () => {
    if (!selectedMeeting) return;
    if (await onSaveMarkdown(selectedMeeting.id)) {
      setExported(true);
      window.setTimeout(() => setExported(false), 1500);
    }
  };

  const renderTabs = () => (
    <div className="meeting-tabs" role="tablist">
      {(
        [
          ["thoughts", "My thoughts"],
          ["transcript", "Transcript"],
          ["summary", "Summary"],
        ] as const
      )
        .filter(
          ([value]) =>
            selectedMeeting ||
            value !== "transcript" ||
            preferences.showLiveTranscript,
        )
        .map(([value, label]) => (
          <button
            key={value}
            className={
              tab === value ? "meeting-tab meeting-tab--active" : "meeting-tab"
            }
            onClick={() => setTab(value)}
            role="tab"
            aria-selected={tab === value}
          >
            {label}
          </button>
        ))}
    </div>
  );

  const renderTranscript = () => (
    <>
      {selectedMeeting && (
        <div className="meeting-transcript-tools">
          <input
            className="form-input"
            value={transcriptSearch}
            onChange={(event) => setTranscriptSearch(event.target.value)}
            placeholder="Search transcript"
            aria-label="Search transcript"
          />
          <button
            className="btn btn-ghost btn-sm"
            onClick={() => void copyTranscript()}
          >
            {copied ? "Copied" : "Copy transcript"}
          </button>
        </div>
      )}
      <div
        className="meeting-transcript"
        ref={transcriptRef}
        onScroll={() => {
          const node = transcriptRef.current;
          if (!node) return;
          setFollowingLive(
            node.scrollHeight - node.scrollTop - node.clientHeight < 32,
          );
        }}
      >
        {turns.length === 0 ? (
          <div className="meeting-listening">
            <div className="meeting-listening-dots" aria-hidden="true">
              <span />
              <span />
              <span />
            </div>
            <p>{phase === "paused" ? "Capture paused" : "Listening…"}</p>
            <span>Transcript text will appear here.</span>
          </div>
        ) : (
          turns.map((turn) => {
            const matchesSearch =
              !transcriptSearch ||
              transcriptMatches.some(
                (match) =>
                  match.speaker === turn.label &&
                  turn.utterances.some((item) =>
                    item.text.includes(match.text),
                  ),
              );
            if (!matchesSearch) return null;
            return (
              <div
                className={`meeting-turn meeting-turn--${turn.owner ? "owner" : "others"}`}
                key={turn.key}
              >
                <div className="meeting-speaker">
                  <span className="meeting-speaker-avatar" aria-hidden="true">
                    {turn.owner ? "Y" : "S"}
                  </span>
                  {selectedMeeting && !turn.owner ? (
                    <input
                      className="meeting-speaker-input"
                      defaultValue={turn.label}
                      list="meeting-speaker-suggestions"
                      aria-label={`Name ${turn.label}`}
                      onBlur={(event) => {
                        const value = event.target.value.trim();
                        saveField({
                          speakerNames: {
                            ...selectedMeeting.speakerNames,
                            [turn.speakerKey]: value,
                          },
                        });
                      }}
                    />
                  ) : (
                    <div className="meeting-speaker-label">
                      <strong>{turn.label}</strong>
                      {turn.utterances.some(
                        (item) => item.isFinal === false,
                      ) && <span>Listening…</span>}
                    </div>
                  )}
                </div>
                <div className="meeting-turn-copy">
                  {turn.utterances.map((item, index) => (
                    <p
                      className={
                        item.isFinal === false
                          ? "meeting-utterance--partial"
                          : undefined
                      }
                      key={`${item.id ?? index}-${item.channel}`}
                    >
                      {item.text}
                    </p>
                  ))}
                </div>
              </div>
            );
          })
        )}
      </div>
      {!followingLive && active && (
        <button
          className="meeting-jump-live"
          onClick={() => {
            transcriptRef.current?.scrollTo({
              top: transcriptRef.current.scrollHeight,
              behavior: "smooth",
            });
            setFollowingLive(true);
          }}
        >
          Jump to live ↓
        </button>
      )}
    </>
  );

  const permissionBanner =
    systemAudioCapability &&
    (systemAudioCapability.permission === "denied" ||
      systemAudioCapability.permission === "unsupported" ||
      (systemAudioCapability.backend === "screen_capture_kit" &&
        systemAudioCapability.permission === "not_determined")) ? (
      <div className="permission-banner meeting-permission-banner">
        <p className="text-danger">
          {systemAudioCapability.permission === "unsupported"
            ? "Meeting audio capture requires macOS 13 or later."
            : `${permissionName} permission is required.`}
        </p>
        <p className="text-secondary">
          GladiaFlow captures Mac audio for live transcription. Audio is never
          saved.
        </p>
        {systemAudioCapability.permission !== "unsupported" && (
          <div className="permission-actions">
            <button
              className="btn btn-primary btn-sm"
              onClick={onGrantPermission}
            >
              Grant permission
            </button>
            <button className="btn btn-ghost btn-sm" onClick={onOpenSettings}>
              Open System Settings
            </button>
            <button
              className="btn btn-ghost btn-sm"
              onClick={onRecheckPermission}
            >
              Re-check
            </button>
          </div>
        )}
      </div>
    ) : null;

  if (!active && !selectedMeeting) {
    return (
      <div className="settings-view meeting-view meeting-library">
        <header className="meeting-library-header">
          <div>
            <h2 className="setup-title">Meetings</h2>
            <p className="setup-desc">
              Your private, locally saved meeting notes.
            </p>
          </div>
          <div className="meeting-library-actions">
            <button
              className="btn btn-ghost"
              aria-expanded={settingsOpen}
              onClick={() => setSettingsOpen((open) => !open)}
            >
              Settings
            </button>
            <button
              className="btn btn-primary meeting-start-button"
              disabled={!permissionReady}
              onClick={onStart}
            >
              <span className="meeting-record-icon" aria-hidden="true" />
              Start meeting
            </button>
          </div>
        </header>
        {settingsOpen && (
          <section
            className="meeting-settings-panel"
            aria-label="Meeting settings"
          >
            <div className="meeting-setting-row">
              <label htmlFor="meeting-maximum-length">
                <strong>Maximum recording length</strong>
                <span>Stop and save automatically at the selected limit.</span>
              </label>
              <select
                id="meeting-maximum-length"
                className="form-input"
                value={preferences.maximumMinutes}
                onChange={(event) =>
                  onPreferencesChange({
                    ...preferences,
                    maximumMinutes: Number(
                      event.target.value,
                    ) as MeetingPreferences["maximumMinutes"],
                  })
                }
              >
                <option value={0}>Off</option>
                <option value={30}>30 minutes</option>
                <option value={60}>60 minutes</option>
                <option value={120}>120 minutes</option>
              </select>
            </div>
            <label className="meeting-setting-row meeting-setting-toggle">
              <span>
                <strong>Show live transcript</strong>
                <span>Display incoming words while a meeting is active.</span>
              </span>
              <input
                type="checkbox"
                checked={preferences.showLiveTranscript}
                onChange={(event) =>
                  onPreferencesChange({
                    ...preferences,
                    showLiveTranscript: event.target.checked,
                  })
                }
              />
            </label>
            <label className="meeting-setting-row meeting-setting-toggle">
              <span>
                <strong>Automatically create local summary</strong>
                <span>Save an on-device extractive summary after capture.</span>
              </span>
              <input
                type="checkbox"
                checked={preferences.automaticallyCreateSummary}
                onChange={(event) =>
                  onPreferencesChange({
                    ...preferences,
                    automaticallyCreateSummary: event.target.checked,
                  })
                }
              />
            </label>
            <div className="meeting-setting-row">
              <label htmlFor="meeting-default-tab">
                <strong>Default saved-meeting tab</strong>
                <span>Choose what opens when you select a saved meeting.</span>
              </label>
              <select
                id="meeting-default-tab"
                className="form-input"
                value={preferences.defaultSavedMeetingTab}
                onChange={(event) =>
                  onPreferencesChange({
                    ...preferences,
                    defaultSavedMeetingTab: event.target.value as MeetingTab,
                  })
                }
              >
                <option value="thoughts">My thoughts</option>
                <option value="transcript">Transcript</option>
                <option value="summary">Summary</option>
              </select>
            </div>
          </section>
        )}
        {permissionBanner}
        {systemAudioCapability?.requiresRestart && (
          <div className="permission-banner">
            Restart GladiaFlow to apply permission.
          </div>
        )}
        {error && (
          <div className="meeting-error" role="alert">
            {error}
          </div>
        )}
        <section className="meeting-calendar">
          <div className="meeting-calendar-heading">
            <div>
              <strong>Today</strong>
              <span>Upcoming events from Calendar stay on this Mac.</span>
            </div>
            {(calendar?.status === "not_determined" ||
              calendar?.status === "write_only") && (
              <button
                className="btn btn-ghost btn-sm"
                disabled={calendarLoading}
                onClick={onConnectCalendar}
              >
                {calendarLoading ? "Connecting…" : "Connect Calendar"}
              </button>
            )}
          </div>
          {calendarLoading && !calendar ? (
            <div className="meeting-calendar-empty">Checking Calendar…</div>
          ) : calendar?.status === "authorized" ? (
            calendar.meetings.length ? (
              <div className="meeting-calendar-list">
                {calendar.meetings.map((meeting) => (
                  <div className="meeting-calendar-item" key={meeting.id}>
                    <div>
                      <strong>{meeting.title || "Untitled event"}</strong>
                      <span>
                        {formatTimeRange(meeting.startAt, meeting.endAt)}
                        {meeting.location ? ` · ${meeting.location}` : ""}
                      </span>
                    </div>
                    <div>
                      {meeting.joinUrl && (
                        <button
                          className="btn btn-ghost btn-sm"
                          onClick={() =>
                            onOpenCalendarMeeting(meeting.joinUrl as string)
                          }
                        >
                          Join
                        </button>
                      )}
                      <button
                        className="btn btn-secondary btn-sm"
                        disabled={!permissionReady}
                        onClick={() => onStartFromCalendar(meeting.title)}
                      >
                        Start
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className="meeting-calendar-empty">
                No upcoming Calendar events today.
              </div>
            )
          ) : calendar?.status === "denied" ||
            calendar?.status === "restricted" ? (
            <div className="meeting-calendar-empty">
              Calendar access is off. Enable it in System Settings to show
              upcoming meetings.
            </div>
          ) : calendar?.status === "unsupported" ? (
            <div className="meeting-calendar-empty">
              Calendar integration requires macOS 14 or later.
            </div>
          ) : (
            <div className="meeting-calendar-empty">
              Connect Calendar to start notes from an upcoming event.
            </div>
          )}
        </section>
        <input
          className="form-input meeting-library-search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search meetings"
          aria-label="Search meetings"
        />
        <div className="meeting-list">
          {loading ? (
            <div className="meeting-empty">Loading meetings…</div>
          ) : visibleMeetings.length === 0 ? (
            <div className="meeting-empty">
              <strong>
                {query ? "No matching meetings" : "No meetings yet"}
              </strong>
              <span>
                {query
                  ? "Try another search."
                  : "Start a meeting to create a searchable local transcript."}
              </span>
            </div>
          ) : (
            visibleMeetings.map((meeting) => (
              <button
                className="meeting-list-item"
                key={meeting.id}
                onClick={() => onSelectMeeting(meeting.id)}
              >
                <span>
                  <strong>{meetingDisplayTitle(meeting)}</strong>
                  <small>{formatDate(meeting.createdAt)}</small>
                </span>
                <span className="meeting-list-meta">
                  {meeting.folder || formatDuration(meeting.duration)}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="settings-view meeting-view meeting-workspace">
      <header className="meeting-workspace-header">
        {selectedMeeting && (
          <button
            className="meeting-back"
            onClick={() => onSelectMeeting(null)}
          >
            ← Meetings
          </button>
        )}
        <input
          className="meeting-title-input"
          value={selectedMeeting ? title : liveTitle}
          onChange={(event) =>
            selectedMeeting
              ? setTitle(event.target.value)
              : onLiveTitleChange(event.target.value)
          }
          onBlur={() => selectedMeeting && saveField({ title })}
          placeholder="Untitled meeting"
          aria-label="Meeting title"
        />
        {selectedMeeting && (
          <div className="meeting-workspace-actions">
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => void copyMarkdown()}
            >
              {copied ? "Copied" : "Copy Markdown"}
            </button>
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => void saveMarkdown()}
            >
              {exported ? "Saved" : "Save .md"}
            </button>
          </div>
        )}
        <div className="meeting-workspace-meta">
          <span
            className={`meeting-status-dot${
              phase === "recording" ? " meeting-status-dot--live" : ""
            }`}
          />
          <span>
            {formatDuration(selectedMeeting?.duration ?? elapsedSeconds)}
          </span>
          <span>
            {selectedMeeting
              ? formatDate(selectedMeeting.createdAt)
              : phase === "paused"
                ? "Paused"
                : phase === "finalizing"
                  ? "Saving…"
                  : "Live"}
          </span>
          {speakerCount > 0 && (
            <span>
              {speakerCount} {speakerCount === 1 ? "speaker" : "speakers"}
            </span>
          )}
        </div>
      </header>
      {error && (
        <div className="meeting-error" role="alert">
          {error}
        </div>
      )}
      <section className="meeting-live-card">
        {selectedMeeting && (
          <>
            <datalist id="meeting-speaker-suggestions">
              {suggestions.speakerNames.map((name) => (
                <option value={name} key={name} />
              ))}
            </datalist>
            <datalist id="meeting-folder-suggestions">
              {suggestions.folders.map((name) => (
                <option value={name} key={name} />
              ))}
            </datalist>
          </>
        )}
        {renderTabs()}
        <div className="meeting-tab-content">
          {tab === "thoughts" && (
            <textarea
              className="meeting-notes"
              value={selectedMeeting ? notes : liveNotes}
              onChange={(event) =>
                selectedMeeting
                  ? setNotes(event.target.value)
                  : onLiveNotesChange(event.target.value)
              }
              placeholder="Write private notes…"
              aria-label="My thoughts"
            />
          )}
          {tab === "transcript" && renderTranscript()}
          {tab === "summary" && (
            <div className="meeting-summary-panel">
              <h3>On-device summary</h3>
              <p className="text-secondary">
                Deterministic extractive summary generated only from this
                transcript.
              </p>
              <div className="meeting-summary-copy">
                {selectedMeeting?.summary ||
                  localSummary ||
                  "Not enough transcript yet."}
              </div>
              {selectedMeeting && !selectedMeeting.summary && localSummary && (
                <button
                  className="btn btn-secondary btn-sm"
                  onClick={() => saveField({ summary: localSummary })}
                >
                  Save summary
                </button>
              )}
              {selectedMeeting && (
                <div className="meeting-qa">
                  <h3>Find relevant excerpts</h3>
                  <p className="text-secondary">
                    Local keyword matching, not generative AI. Results are
                    direct transcript excerpts.
                  </p>
                  <form
                    onSubmit={(event) => {
                      event.preventDefault();
                      setAskedQuestion(question);
                    }}
                  >
                    <input
                      className="form-input"
                      value={question}
                      onChange={(event) => setQuestion(event.target.value)}
                      placeholder="What did we say about the launch?"
                    />
                    <button className="btn btn-secondary btn-sm" type="submit">
                      Find
                    </button>
                  </form>
                  {askedQuestion &&
                    (answerMatches.length ? (
                      <ul>
                        {answerMatches.map((match, index) => (
                          <li key={`${match.speaker}-${index}`}>
                            <strong>{match.speaker}:</strong> {match.text}
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="text-secondary">
                        No relevant excerpts found.
                      </p>
                    ))}
                </div>
              )}
            </div>
          )}
        </div>
        {selectedMeeting ? (
          <footer className="meeting-detail-footer">
            <label>
              Folder
              <input
                className="form-input"
                value={folder}
                list="meeting-folder-suggestions"
                onChange={(event) => setFolder(event.target.value)}
                onBlur={() => saveField({ folder })}
                placeholder="No folder"
              />
            </label>
            <div>
              {deleteConfirm ? (
                <>
                  <span>Delete this meeting?</span>
                  <button
                    className="btn btn-danger btn-sm"
                    onClick={() => void onDeleteMeeting(selectedMeeting.id)}
                  >
                    Delete
                  </button>
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => setDeleteConfirm(false)}
                  >
                    Cancel
                  </button>
                </>
              ) : (
                <button
                  className="btn btn-ghost btn-sm meeting-delete"
                  onClick={() => setDeleteConfirm(true)}
                >
                  Delete meeting
                </button>
              )}
            </div>
          </footer>
        ) : (
          <footer className="meeting-controls">
            {phase === "recording" && (
              <button className="meeting-stop-button" onClick={onPause}>
                Pause
              </button>
            )}
            {phase === "paused" && (
              <button className="meeting-stop-button" onClick={onResume}>
                Resume
              </button>
            )}
            <button
              className="meeting-stop-button"
              disabled={phase === "starting" || phase === "finalizing"}
              onClick={onStop}
            >
              <span className="meeting-stop-icon" aria-hidden="true" />
              {phase === "finalizing" ? "Finishing up…" : "Stop"}
            </button>
            <span className="meeting-controls-note">
              Audio is streamed for transcription and never saved.
            </span>
          </footer>
        )}
      </section>
    </div>
  );
}
