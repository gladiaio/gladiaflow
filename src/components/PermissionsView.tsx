import {
  type AccessibilityState,
  accessibilityHelperText,
  accessibilityRowClass,
  accessibilityStatusLabel,
  isAccessibilityGranted,
} from "../lib/accessibilityPermission";
import { GladiaIcon } from "./GladiaIcon";

export function PermissionsView({
  permissions,
  isMac,
  onOpenAccessibilitySettings,
  onGrantMicrophone,
  onOpenMicSettings,
  onRecheck,
}: {
  permissions: {
    accessibilityState: AccessibilityState | null;
    microphone: string | null;
  };
  isMac: boolean;
  onOpenAccessibilitySettings: () => void;
  onGrantMicrophone: () => void;
  onOpenMicSettings: () => void;
  onRecheck: () => void;
}) {
  const axState = permissions.accessibilityState;
  const axGranted = isAccessibilityGranted(axState);
  const axRowClass = accessibilityRowClass(axState);
  const micGranted = permissions.microphone === "authorized";

  return (
    <div className="setup-step setup-step-center">
      <h2 className="setup-title">Permissions Required</h2>
      <p className="setup-desc">
        GladiaFlow needs these permissions to work. Enable each one, then click
        Re-check.
      </p>
      <div className="permissions-list">
        <div
          className={`permission-row ${axRowClass} permission-row--accessibility`}
        >
          <div className="permission-icon">
            <GladiaIcon name="keyboard" size={16} />
          </div>
          <div className="permission-info">
            <span className="permission-name">Accessibility</span>
            <span className="permission-why">
              {accessibilityHelperText(axState, isMac)}
            </span>
            <span className="permission-status-label">
              {accessibilityStatusLabel(axState, isMac)}
            </span>
          </div>
          {!axGranted && isMac && (
            <button
              className="btn btn-primary btn-sm"
              onClick={onOpenAccessibilitySettings}
            >
              Open Settings
            </button>
          )}
        </div>
        <div
          className={`permission-row ${micGranted ? "granted" : "needed"} permission-row--microphone`}
        >
          <div className="permission-icon">
            <GladiaIcon name="microphone" size={16} />
          </div>
          <div className="permission-info">
            <span className="permission-name">Microphone</span>
            <span className="permission-why">
              {micGranted
                ? "Granted"
                : permissions.microphone === "denied"
                  ? "Denied — open Settings to enable"
                  : "Click Grant to allow microphone access"}
            </span>
          </div>
          {!micGranted &&
            (permissions.microphone === "denied" ? (
              <button
                className="btn btn-ghost btn-sm"
                onClick={onOpenMicSettings}
              >
                Open Settings
              </button>
            ) : (
              <button
                className="btn btn-primary btn-sm"
                onClick={onGrantMicrophone}
              >
                Grant
              </button>
            ))}
        </div>
      </div>
      <div className="setup-nav setup-nav-center">
        <button className="btn btn-primary" onClick={onRecheck}>
          Re-check Permissions
        </button>
      </div>
    </div>
  );
}
