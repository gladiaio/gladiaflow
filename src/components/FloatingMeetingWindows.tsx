import { useEffect, useState } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import type { CalendarMeeting, MeetingPhase } from "../types";
import { MeetingCalendarPrompt } from "./MeetingCalendarPrompt";

export type FloatingMeetingAction =
  | { action: "open" }
  | { action: "pause" }
  | { action: "resume" }
  | { action: "stop" }
  | {
      action: "calendar-start";
      meeting: CalendarMeeting;
      join: boolean;
    }
  | { action: "calendar-dismiss"; meetingId: string };

const sendAction = (action: FloatingMeetingAction) =>
  emitTo("main", "meeting-floating-action", action);

export function FloatingMeetingController() {
  const [phase, setPhase] = useState<MeetingPhase>("idle");

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    void listen<{ phase: MeetingPhase }>(
      "meeting-controller-state",
      ({ payload }) => setPhase(payload.phase),
    ).then((unlisten) => {
      cleanup = unlisten;
      void emitTo("main", "meeting-floating-ready", {
        surface: "meeting-controller",
      });
    });
    return () => cleanup?.();
  }, []);

  const paused = phase === "paused";
  const busy = phase === "starting" || phase === "finalizing";

  return (
    <div
      className="floating-meeting-controller floating-surface"
      data-tauri-drag-region
      onDoubleClick={() => void sendAction({ action: "open" })}
    >
      <button
        className="floating-meeting-open"
        aria-label={`Open meeting — ${paused ? "paused" : busy ? "working" : "recording"}`}
        onClick={() => void sendAction({ action: "open" })}
      >
        <span className="floating-meeting-wave" aria-hidden="true">
          <i />
          <i />
          <i />
          <i />
        </span>
      </button>
      <button
        className="floating-meeting-toggle"
        disabled={busy}
        aria-label={paused ? "Resume meeting" : "Pause meeting"}
        onClick={() => void sendAction({ action: paused ? "resume" : "pause" })}
      >
        {paused ? "▶" : "Ⅱ"}
      </button>
      <button
        className="floating-meeting-stop"
        disabled={phase === "finalizing"}
        aria-label="Stop meeting"
        onClick={() => void sendAction({ action: "stop" })}
      >
        <span aria-hidden="true" />
      </button>
    </div>
  );
}

export function FloatingMeetingReminder() {
  const [meeting, setMeeting] = useState<CalendarMeeting | null>(null);

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    void listen<{ meeting: CalendarMeeting }>(
      "meeting-reminder-state",
      ({ payload }) => setMeeting(payload.meeting),
    ).then((unlisten) => {
      cleanup = unlisten;
      void emitTo("main", "meeting-floating-ready", {
        surface: "meeting-reminder",
      });
    });
    return () => cleanup?.();
  }, []);

  if (!meeting) return <div className="floating-surface" />;

  return (
    <div className="floating-meeting-reminder floating-surface">
      <MeetingCalendarPrompt
        meeting={meeting}
        onJoinAndStart={() =>
          void sendAction({
            action: "calendar-start",
            meeting,
            join: true,
          })
        }
        onStart={() =>
          void sendAction({
            action: "calendar-start",
            meeting,
            join: false,
          })
        }
        onDismiss={() =>
          void sendAction({
            action: "calendar-dismiss",
            meetingId: meeting.id,
          })
        }
      />
    </div>
  );
}
