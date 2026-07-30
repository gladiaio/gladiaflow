import type { CustomVocabEntry } from "../types";

export const RECOMMENDED_INTENSITY_MIN = 0.4;
export const RECOMMENDED_INTENSITY_MAX = 0.6;

export type IntensityGuidance = {
  label: "Low" | "Recommended" | "High";
  tone: "neutral" | "recommended" | "warning";
  message: string;
};

export function getIntensityGuidance(intensity: number): IntensityGuidance {
  if (intensity > RECOMMENDED_INTENSITY_MAX) {
    return {
      label: "High",
      tone: "warning",
      message: "Higher values can cause unrelated words to be replaced.",
    };
  }
  if (intensity >= RECOMMENDED_INTENSITY_MIN) {
    return {
      label: "Recommended",
      tone: "recommended",
      message: "Gladia recommends values between 0.4 and 0.6.",
    };
  }
  return {
    label: "Low",
    tone: "neutral",
    message: "Lower values apply a gentler vocabulary boost.",
  };
}

export function validateVocabularyTerm(
  entries: CustomVocabEntry[],
  value: string,
  editingIndex: number | null,
): string | null {
  const normalized = value.trim().toLowerCase();
  if (!normalized) return "Enter a term.";
  const duplicate = entries.some(
    (entry, index) =>
      index !== editingIndex && entry.value.trim().toLowerCase() === normalized,
  );
  return duplicate ? "This term is already in your vocabulary." : null;
}

export function normalizePronunciationCandidate(
  pronunciations: string[],
  value: string,
): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const duplicate = pronunciations.some(
    (pronunciation) => pronunciation.toLowerCase() === trimmed.toLowerCase(),
  );
  return duplicate ? null : trimmed;
}

export function clampPage(page: number, itemCount: number, pageSize: number) {
  const totalPages = Math.max(1, Math.ceil(itemCount / pageSize));
  return Math.min(Math.max(1, page), totalPages);
}
