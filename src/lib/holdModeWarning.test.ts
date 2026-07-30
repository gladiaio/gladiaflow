import { describe, expect, it } from "vitest";
import { updateHoldModeWarningStreak } from "./holdModeWarning";

const update = (
  currentStreak: number,
  durationSeconds = 1,
  transcript = "",
  activationMode: "toggle" | "push-to-talk" = "push-to-talk",
) =>
  updateHoldModeWarningStreak({
    currentStreak,
    durationSeconds,
    transcript,
    activationMode,
  });

describe("updateHoldModeWarningStreak", () => {
  it("warns after three consecutive short empty hold dictations", () => {
    expect(update(0, 1.99)).toEqual({ streak: 1, shouldWarn: false });
    expect(update(1, 1.99)).toEqual({ streak: 2, shouldWarn: false });
    expect(update(2, 1.99)).toEqual({ streak: 0, shouldWarn: true });
  });

  it("resets the streak after a successful or longer dictation", () => {
    expect(update(2, 1, "hello")).toEqual({
      streak: 0,
      shouldWarn: false,
    });
    expect(update(2, 2.01)).toEqual({ streak: 0, shouldWarn: false });
  });

  it("treats two seconds as outside the short-session threshold", () => {
    expect(update(2, 2)).toEqual({ streak: 0, shouldWarn: false });
  });

  it("never builds a streak in toggle mode", () => {
    expect(update(2, 1, "", "toggle")).toEqual({
      streak: 0,
      shouldWarn: false,
    });
  });
});
