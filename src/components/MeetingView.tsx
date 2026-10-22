import { useEffect, useMemo, useRef, useState } from "react";
import type { MeetingUtterance, SystemAudioCapability } from "../types";

const speakerLabel = (utterance: MeetingUtterance) =>
  utterance.owner
    ? "You"
    : utterance.speaker === undefined
      ? "Others"
      : `Speaker ${utterance.speaker + 1}`;

interface MeetingTurn {
  key: string;
  label: string;
  owner: boolean;
  utterances: MeetingUtterance[];
}

export const groupMeetingUtterances = (
  utterances: MeetingUtterance[],
): MeetingTurn[] =>
  utterances.reduce<MeetingTurn[]>((turns, utterance, index) => {
    const label = speakerLabel(utterance);
    const speakerKey = utterance.owner
      ? "owner"
      : utterance.speaker === undefined
        ? "others"
        : `speaker-${utterance.speaker}`;
    const previous = turns[turns.length - 1];
    if (previous?.key.startsWith(`${speakerKey}:`)) {
      previous.utterances.push(utterance);
      return turns;
    }
    turns.push({
      key: `${speakerKey}:${utterance.id ?? index}`,
      label,
      owner: utterance.owner,
      utterances: [utterance],
    });
    return turns;
  }, []);

const formatDuration = (seconds: number) => {
  const minutes = Math.floor(seconds / 60);
  return `${minutes.toString().padStart(2, "0")}:${(seconds % 60)
    .toString()
    .padStart(2, "0")}`;
};

export function MeetingView({
  phase,
  systemAudioCapability,
  utterances,
  error,
  onStart,
  onStop,
  onGrantPermission,
  onOpenSettings,
  onRecheckPermission,
}: {
  phase: "idle" | "starting" | "recording" | "finalizing";
  systemAudioCapability: SystemAudioCapability | null;
  utterances: MeetingUtterance[];
  error: string | null;
  onStart: () => void;
  onStop: () => void;
  onGrantPermission: () => void;
  onOpenSettings: () => void;
  onRecheckPermission: () => void;
}) {
  const active = phase !== "idle";
  const permissionReady =
    systemAudioCapability?.permission === "authorized" ||
    (systemAudioCapability?.backend === "core_audio_process_tap" &&
      systemAudioCapability.permission === "not_determined");
  const permissionName =
    systemAudioCapability?.backend === "screen_capture_kit"
      ? "Screen Recording"
      : "System Audio Recording";
  const recording = phase === "recording";
  const turns = useMemo(() => groupMeetingUtterances(utterances), [utterances]);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const startedAtRef = useRef<number | null>(null);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [followingLive, setFollowingLive] = useState(true);

  useEffect(() => {
    if (recording && startedAtRef.current === null) {
      startedAtRef.current = Date.now();
    }
    if (phase === "idle") {
      startedAtRef.current = null;
      setElapsedSeconds(0);
      setFollowingLive(true);
      return;
    }
    if (!recording) return;

    const updateElapsed = () => {
      if (startedAtRef.current !== null) {
        setElapsedSeconds(
          Math.max(0, Math.floor((Date.now() - startedAtRef.current) / 1000)),
        );
      }
    };
    updateElapsed();
    const interval = window.setInterval(updateElapsed, 1000);
    return () => window.clearInterval(interval);
  }, [phase, recording]);

  useEffect(() => {
    if (!followingLive) return;
    const transcript = transcriptRef.current;
    if (transcript) transcript.scrollTop = transcript.scrollHeight;
  }, [utterances, followingLive]);

  const handleTranscriptScroll = () => {
    const transcript = transcriptRef.current;
    if (!transcript) return;
    const distanceFromBottom =
      transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight;
    setFollowingLive(distanceFromBottom < 32);
  };

  const jumpToLive = () => {
    const transcript = transcriptRef.current;
    if (transcript) {
      transcript.scrollTo({ top: transcript.scrollHeight, behavior: "smooth" });
    }
    setFollowingLive(true);
  };
  const speakerAvatar = (turn: MeetingTurn) => {
    if (turn.owner) return "Y";
    const number = turn.label.match(/^Speaker (\d+)$/)?.[1];
    return number ? `S${number}` : "O";
  };

  return (
    <div className="settings-view meeting-view">
      <div className="meeting-header">
        <div>
          <h2 className="setup-title">Meeting recording</h2>
          <p className="setup-desc">
            Live notes from your microphone and Mac audio.
          </p>
        </div>
        {active && (
          <div className="meeting-session-status" aria-live="polite">
            <span
              className={`meeting-status-dot${recording ? " meeting-status-dot--live" : ""}`}
              aria-hidden="true"
            />
            <span className="meeting-status-label">
              {phase === "starting"
                ? "Starting…"
                : phase === "finalizing"
                  ? "Finishing up…"
                  : "Recording"}
            </span>
            <span className="meeting-timer">
              {formatDuration(elapsedSeconds)}
            </span>
          </div>
        )}
      </div>

      {systemAudioCapability?.permission === "unsupported" && (
        <div className="permission-banner">
          <p className="text-danger">
            Meeting audio capture requires macOS 13 or later.
          </p>
        </div>
      )}

      {systemAudioCapability &&
        (systemAudioCapability.permission === "denied" ||
          (systemAudioCapability.backend === "screen_capture_kit" &&
            systemAudioCapability.permission === "not_determined")) && (
          <div className="permission-banner">
            <p className="text-danger">
              {permissionName} permission is required.
            </p>
            <p className="text-secondary">
              It lets GladiaFlow capture Mac audio. No screen video or audio is
              stored.
            </p>
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
          </div>
        )}

      {systemAudioCapability?.requiresRestart && (
        <div className="permission-banner">
          <p className="text-secondary">
            Restart GladiaFlow to finish applying the audio permission.
          </p>
        </div>
      )}

      {error && (
        <div className="meeting-error" role="alert">
          {error}
        </div>
      )}

      {!active && utterances.length === 0 ? (
        <section className="meeting-start-card">
          <div className="meeting-wave" aria-hidden="true">
            <span />
            <span />
            <span />
            <span />
            <span />
          </div>
          <div className="meeting-start-copy">
            <h3>Capture the whole conversation</h3>
            <p>
              GladiaFlow separates your voice from everyone speaking through
              your Mac, without storing the audio.
            </p>
          </div>
          <div className="meeting-channel-legend" aria-label="Audio channels">
            <span>
              <i className="meeting-channel-dot meeting-channel-dot--owner" />
              You
            </span>
            <span>
              <i className="meeting-channel-dot" />
              Remote speakers
            </span>
          </div>
          <button
            className="btn btn-primary meeting-start-button"
            disabled={!permissionReady}
            onClick={onStart}
          >
            <span className="meeting-record-icon" aria-hidden="true" />
            Start meeting
          </button>
        </section>
      ) : (
        <section className="meeting-live-card">
          <div className="meeting-live-toolbar">
            <div>
              <h3>{active ? "Live transcript" : "Meeting transcript"}</h3>
              <p>
                {active
                  ? "Speaker labels settle when each phrase is finalized."
                  : "The completed transcript is saved in History."}
              </p>
            </div>
            {recording && (
              <span className="meeting-live-badge">
                <span aria-hidden="true" />
                Live
              </span>
            )}
          </div>

          <div
            ref={transcriptRef}
            className="meeting-transcript"
            aria-live="polite"
            onScroll={handleTranscriptScroll}
          >
            {turns.length === 0 ? (
              <div className="meeting-listening">
                <div className="meeting-listening-dots" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </div>
                <p>
                  {phase === "finalizing"
                    ? "Finishing the transcript…"
                    : "Listening…"}
                </p>
                <span>Start speaking or play audio on your Mac.</span>
              </div>
            ) : (
              turns.map((turn) => (
                <div
                  className={`meeting-turn meeting-turn--${turn.owner ? "owner" : "others"}`}
                  key={turn.key}
                >
                  <div className="meeting-speaker">
                    <span className="meeting-speaker-avatar" aria-hidden="true">
                      {speakerAvatar(turn)}
                    </span>
                    <strong>{turn.label}</strong>
                  </div>
                  <div className="meeting-turn-copy">
                    {turn.utterances.map((utterance, index) => (
                      <p
                        className={
                          utterance.isFinal === false
                            ? "meeting-utterance--partial"
                            : undefined
                        }
                        key={`${utterance.id ?? index}-${utterance.channel}`}
                      >
                        {utterance.text}
                      </p>
                    ))}
                  </div>
                </div>
              ))
            )}
          </div>

          {!followingLive && (
            <button className="meeting-jump-live" onClick={jumpToLive}>
              Jump to live
              <span aria-hidden="true">↓</span>
            </button>
          )}

          <div className="meeting-controls">
            {phase === "recording" ? (
              <button
                className="meeting-stop-button"
                onClick={onStop}
                aria-label="Stop meeting"
              >
                <span className="meeting-stop-icon" aria-hidden="true" />
                Stop meeting
              </button>
            ) : phase === "starting" || phase === "finalizing" ? (
              <button className="meeting-stop-button" disabled>
                <span className="meeting-control-spinner" aria-hidden="true" />
                {phase === "starting" ? "Starting…" : "Finishing up…"}
              </button>
            ) : (
              <button
                className="meeting-stop-button"
                disabled={!permissionReady}
                onClick={onStart}
              >
                <span className="meeting-record-icon" aria-hidden="true" />
                New meeting
              </button>
            )}
            <span className="meeting-controls-note">
              Audio is streamed for transcription and never saved locally.
            </span>
          </div>
        </section>
      )}
    </div>
  );
}
