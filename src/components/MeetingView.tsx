import type { MeetingUtterance, SystemAudioCapability } from "../types";

const speakerLabel = (utterance: MeetingUtterance) =>
  utterance.owner
    ? "You"
    : utterance.speaker === undefined
      ? "Others"
      : `Speaker ${utterance.speaker + 1}`;

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

  return (
    <div className="settings-view meeting-view">
      <div className="settings-header">
        <div>
          <h2 className="setup-title">Meeting recording</h2>
          <p className="setup-desc">
            Your microphone is labeled You. Voices from Mac audio are separated
            in real time.
          </p>
        </div>
        <div className="meeting-actions">
          {phase === "recording" ? (
            <button className="btn btn-primary" onClick={onStop}>
              Stop meeting
            </button>
          ) : (
            <button
              className="btn btn-primary"
              disabled={active || !permissionReady}
              onClick={onStart}
            >
              {phase === "starting"
                ? "Starting…"
                : phase === "finalizing"
                  ? "Finalizing…"
                  : "Start meeting"}
            </button>
          )}
        </div>
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

      {error && <p className="text-danger">{error}</p>}

      <div className="meeting-transcript" aria-live="polite">
        {utterances.length === 0 ? (
          <p className="form-footnote">
            {phase === "recording"
              ? "Listening to both audio channels…"
              : "The live transcript will appear here."}
          </p>
        ) : (
          utterances.map((utterance, index) => (
            <div
              className={`meeting-utterance meeting-utterance--${utterance.owner ? "owner" : "others"}`}
              key={`${utterance.id ?? index}-${utterance.channel}`}
            >
              <strong>{speakerLabel(utterance)}</strong>
              <span>{utterance.text}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
