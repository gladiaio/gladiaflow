import { useState } from "react";
import type { CalendarMeeting } from "../types";

export function MeetingCalendarPrompt({
  meeting,
  onJoinAndStart,
  onStart,
  onDismiss,
}: {
  meeting: CalendarMeeting;
  onJoinAndStart: () => void;
  onStart: () => void;
  onDismiss: () => void;
}) {
  const [expanded, setExpanded] = useState(false);

  return (
    <aside className="meeting-calendar-prompt" aria-label="Meeting starting">
      <button
        className="meeting-calendar-prompt-close"
        aria-label="Dismiss meeting reminder"
        onClick={onDismiss}
      >
        ×
      </button>
      <div className="meeting-calendar-prompt-copy">
        <span className="meeting-calendar-prompt-camera" aria-hidden="true" />
        <div>
          <strong>{meeting.title || "Meeting"}</strong>
          <span>
            <i aria-hidden="true" />
            Now
          </span>
        </div>
      </div>
      <div className="meeting-calendar-prompt-action">
        <button
          className="meeting-calendar-prompt-main"
          onClick={meeting.joinUrl ? onJoinAndStart : onStart}
        >
          <span className="meeting-calendar-prompt-logo" aria-hidden="true">
            |||
          </span>
          <span>
            <strong>
              {meeting.joinUrl ? "Join meeting" : "Start meeting notes"}
            </strong>
            <small>
              {meeting.joinUrl ? "& start Meeting" : "Capture audio locally"}
            </small>
          </span>
        </button>
        <button
          className="meeting-calendar-prompt-expand"
          aria-label="More meeting actions"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? "⌄" : "⌃"}
        </button>
        {expanded && (
          <div className="meeting-calendar-prompt-menu">
            <button onClick={onStart}>Start without joining</button>
            <button onClick={onDismiss}>Dismiss for this meeting</button>
          </div>
        )}
      </div>
    </aside>
  );
}
