import { describe, expect, it } from "vitest";
import type { CustomVocabEntry } from "../types";
import {
  clampPage,
  getIntensityGuidance,
  normalizePronunciationCandidate,
  validateVocabularyTerm,
} from "./vocabularyEditor";

const entries: CustomVocabEntry[] = [
  { value: "GladiaFlow", intensity: 0.4 },
  { value: "WebSocket", intensity: 0.5 },
];

describe("validateVocabularyTerm", () => {
  it("requires a non-empty term", () => {
    expect(validateVocabularyTerm(entries, "  ", null)).toBe("Enter a term.");
  });

  it("rejects case-insensitive duplicates", () => {
    expect(validateVocabularyTerm(entries, " gladiaflow ", null)).toBe(
      "This term is already in your vocabulary.",
    );
  });

  it("allows the currently edited term", () => {
    expect(validateVocabularyTerm(entries, "gladiaflow", 0)).toBeNull();
  });
});

describe("normalizePronunciationCandidate", () => {
  it("trims a unique pronunciation", () => {
    expect(
      normalizePronunciationCandidate(["gladioflow"], " gladiaflaw "),
    ).toBe("gladiaflaw");
  });

  it("rejects empty and case-insensitive duplicate pronunciations", () => {
    expect(
      normalizePronunciationCandidate(["Gladioflow"], "gladioflow"),
    ).toBeNull();
    expect(normalizePronunciationCandidate([], "  ")).toBeNull();
  });
});

describe("getIntensityGuidance", () => {
  it("marks the inclusive 0.4–0.6 band as recommended", () => {
    expect(getIntensityGuidance(0.4).tone).toBe("recommended");
    expect(getIntensityGuidance(0.6).tone).toBe("recommended");
  });

  it("warns above 0.6 without changing the value", () => {
    expect(getIntensityGuidance(0.65)).toMatchObject({
      label: "High",
      tone: "warning",
    });
  });
});

describe("clampPage", () => {
  it("clamps after removing the final page", () => {
    expect(clampPage(3, 8, 4)).toBe(2);
    expect(clampPage(2, 0, 4)).toBe(1);
  });
});
