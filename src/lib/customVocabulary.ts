import { invoke } from "@tauri-apps/api/core";
import type { CustomVocabEntry } from "../types";

export const DEFAULT_VOCAB_INTENSITY = 0.5;

export const DEFAULT_CUSTOM_VOCABULARY: CustomVocabEntry[] = [
  {
    value: "gladiaflow",
    pronunciations: ["gladioflow", "gladiaflaw"],
    intensity: 0.4,
  },
];

const CSV_HEADER = "value,intensity,pronunciations";

export function clampVocabIntensity(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_VOCAB_INTENSITY;
  return Math.min(1, Math.max(0, value));
}

export function normalizeVocabEntry(
  entry: Partial<CustomVocabEntry> & { value: string },
): CustomVocabEntry {
  const value = entry.value.trim();
  const pronunciations = entry.pronunciations
    ?.map((pronunciation) => pronunciation.trim())
    .filter(Boolean);
  const normalized: CustomVocabEntry = {
    value,
    intensity: clampVocabIntensity(entry.intensity ?? DEFAULT_VOCAB_INTENSITY),
  };
  if (pronunciations && pronunciations.length > 0) {
    normalized.pronunciations = pronunciations;
  }
  if (entry.language?.trim()) {
    normalized.language = entry.language.trim();
  }
  return normalized;
}

function escapeCsvField(value: string): string {
  if (/[",\n\r]/.test(value)) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

function splitCsvLine(line: string): string[] {
  const parts: string[] = [];
  let current = "";
  let inQuotes = false;

  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === '"') {
      if (inQuotes && line[index + 1] === '"') {
        current += '"';
        index += 1;
      } else {
        inQuotes = !inQuotes;
      }
      continue;
    }
    if (char === "," && !inQuotes) {
      parts.push(current);
      current = "";
      continue;
    }
    current += char;
  }

  parts.push(current);
  return parts.map((part) => part.trim());
}

function parsePronunciationsCsvField(text: string): string[] | undefined {
  const pronunciations = text
    .split("|")
    .map((part) => part.trim())
    .filter(Boolean);
  return pronunciations.length > 0 ? pronunciations : undefined;
}

export function parseVocabularyImport(text: string): CustomVocabEntry[] {
  const lines = text.trim().split(/\r?\n/).filter(Boolean);
  if (lines.length === 0) return [];

  let startIndex = 0;
  const headerParts = splitCsvLine(lines[0]);
  if (headerParts[0]?.toLowerCase() === "value") {
    startIndex = 1;
  }

  const entries: CustomVocabEntry[] = [];
  for (let index = startIndex; index < lines.length; index += 1) {
    const parts = splitCsvLine(lines[index]);
    const value = parts[0]?.replace(/^"|"$/g, "").trim();
    if (!value) continue;

    const intensity = parts[1]
      ? Number.parseFloat(parts[1])
      : DEFAULT_VOCAB_INTENSITY;
    const pronunciations = parts[2]
      ? parsePronunciationsCsvField(parts[2].replace(/^"|"$/g, ""))
      : undefined;

    entries.push(normalizeVocabEntry({ value, intensity, pronunciations }));
  }

  return entries;
}

export function formatVocabularyExport(entries: CustomVocabEntry[]): string {
  const rows = [CSV_HEADER];
  for (const entry of entries.map((item) => normalizeVocabEntry(item))) {
    const pronunciations = entry.pronunciations?.join("|") ?? "";
    rows.push(
      [
        escapeCsvField(entry.value),
        String(entry.intensity ?? DEFAULT_VOCAB_INTENSITY),
        escapeCsvField(pronunciations),
      ].join(","),
    );
  }
  return `${rows.join("\n")}\n`;
}

export function mergeVocabulary(
  existing: CustomVocabEntry[],
  imported: CustomVocabEntry[],
): CustomVocabEntry[] {
  const importByKey = new Map(
    imported.map((entry) => [entry.value.toLowerCase(), entry]),
  );
  const seen = new Set<string>();
  const merged = existing.map((entry) => {
    const key = entry.value.toLowerCase();
    seen.add(key);
    return importByKey.get(key) ?? entry;
  });

  for (const entry of imported) {
    const key = entry.value.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(entry);
  }

  return merged;
}

export async function downloadVocabularyExport(entries: CustomVocabEntry[]) {
  const csv = formatVocabularyExport(entries);
  await invoke("export_vocabulary_csv", { csv });
}

export async function importVocabularyFromFile(): Promise<
  CustomVocabEntry[] | null
> {
  const csv = await invoke<string | null>("import_vocabulary_csv");
  if (csv == null) return null;
  return parseVocabularyImport(csv);
}
