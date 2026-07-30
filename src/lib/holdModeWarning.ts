export const SHORT_EMPTY_DICTATION_SECONDS = 2;
export const SHORT_EMPTY_DICTATION_LIMIT = 3;

export interface HoldModeWarningResult {
  streak: number;
  shouldWarn: boolean;
}

export function updateHoldModeWarningStreak({
  currentStreak,
  activationMode,
  durationSeconds,
  transcript,
}: {
  currentStreak: number;
  activationMode: "toggle" | "push-to-talk";
  durationSeconds: number;
  transcript: string;
}): HoldModeWarningResult {
  const isShortEmptyHoldDictation =
    activationMode === "push-to-talk" &&
    durationSeconds < SHORT_EMPTY_DICTATION_SECONDS &&
    transcript.trim().length === 0;

  if (!isShortEmptyHoldDictation) {
    return { streak: 0, shouldWarn: false };
  }

  const streak = currentStreak + 1;
  if (streak >= SHORT_EMPTY_DICTATION_LIMIT) {
    return { streak: 0, shouldWarn: true };
  }

  return { streak, shouldWarn: false };
}
