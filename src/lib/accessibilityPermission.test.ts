import { describe, expect, it } from "vitest";
import {
  accessibilityRowClass,
  isAccessibilityGranted,
} from "./accessibilityPermission";

describe("accessibilityPermission", () => {
  it("isAccessibilityGranted only for granted_working", () => {
    expect(isAccessibilityGranted("granted_working")).toBe(true);
    expect(isAccessibilityGranted("denied")).toBe(false);
    expect(isAccessibilityGranted("not_determined")).toBe(false);
    expect(isAccessibilityGranted(null)).toBe(false);
  });

  it("accessibilityRowClass maps states to row styles", () => {
    expect(accessibilityRowClass("granted_working")).toBe("granted");
    expect(accessibilityRowClass("denied")).toBe("needed");
    expect(accessibilityRowClass(null)).toBe("needed");
  });
});
