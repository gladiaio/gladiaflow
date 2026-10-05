import { describe, expect, it } from "vitest";
import { resolveHotkeyPress } from "./dictationHotkey";

const press = (
  activationMode: "toggle" | "push-to-talk",
  state: Partial<{
    isProcessing: boolean;
    isInitializing: boolean;
    isRecording: boolean;
  }> = {},
) =>
  resolveHotkeyPress({
    activationMode,
    isProcessing: false,
    isInitializing: false,
    isRecording: false,
    ...state,
  });

describe("resolveHotkeyPress", () => {
  it("queues a restart while finalizing, in both modes", () => {
    expect(press("toggle", { isProcessing: true })).toBe("queue-restart");
    expect(press("push-to-talk", { isProcessing: true })).toBe("queue-restart");
  });

  it("starts from idle in both modes", () => {
    expect(press("toggle")).toBe("start");
    expect(press("push-to-talk")).toBe("start");
  });

  it("uses the early-stop path when toggled off during session init", () => {
    expect(press("toggle", { isRecording: true, isInitializing: true })).toBe(
      "stop-during-init",
    );
  });

  it("stops a fully started toggle dictation", () => {
    expect(press("toggle", { isRecording: true })).toBe("stop");
  });

  it("ignores repeated presses in hold mode while active", () => {
    expect(
      press("push-to-talk", { isRecording: true, isInitializing: true }),
    ).toBe("ignore");
    expect(press("push-to-talk", { isRecording: true })).toBe("ignore");
  });
});
