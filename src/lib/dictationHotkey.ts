export type HotkeyPressAction =
  "queue-restart" | "stop-during-init" | "stop" | "start" | "ignore";

export function resolveHotkeyPress({
  activationMode,
  isProcessing,
  isInitializing,
  isRecording,
}: {
  activationMode: "toggle" | "push-to-talk";
  isProcessing: boolean;
  isInitializing: boolean;
  isRecording: boolean;
}): HotkeyPressAction {
  // Rapid re-press while the previous utterance is still finalizing: queue a
  // restart for as soon as processing clears (instead of dropping the press).
  if (isProcessing) return "queue-restart";

  if (activationMode === "push-to-talk") {
    return isRecording || isInitializing ? "ignore" : "start";
  }

  // Toggle: a second press before the Gladia session exists must go through
  // the early-stop path, otherwise the start flow is abandoned mid-init and
  // no session-ended event ever clears the finalizing state.
  if (isInitializing) return "stop-during-init";
  return isRecording ? "stop" : "start";
}
