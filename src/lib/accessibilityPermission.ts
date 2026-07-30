export type AccessibilityState =
  | "not_determined"
  | "denied"
  | "granted_working";

export function isAccessibilityGranted(
  state: AccessibilityState | null,
): boolean {
  return state === "granted_working";
}

export function accessibilityStatusLabel(
  state: AccessibilityState | null,
  isMac: boolean,
): string {
  if (!isMac) return "Granted";
  switch (state) {
    case "granted_working":
      return "Granted";
    case "not_determined":
    case "denied":
    default:
      return "Not granted";
  }
}

export function accessibilityHelperText(
  state: AccessibilityState | null,
  isMac: boolean,
): string {
  if (!isMac) {
    return "No additional accessibility permission needed on this platform";
  }
  switch (state) {
    case "granted_working":
      return "Granted";
    case "denied":
    case "not_determined":
    default:
      return "Open Settings → Privacy & Security → Accessibility and enable GladiaFlow. Return here when done — permissions update automatically.";
  }
}

export function accessibilityRowClass(
  state: AccessibilityState | null,
): "granted" | "needed" {
  if (state === "granted_working") return "granted";
  return "needed";
}
