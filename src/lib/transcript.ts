export const countWords = (text: string): number => {
  const trimmed = text.trim();
  if (!trimmed) return 0;
  return trimmed.split(/\s+/).filter(Boolean).length;
};

export const buildDisplayTranscript = (
  finalText: string,
  partialText: string,
): string => {
  const finalTrimmed = finalText.trim();
  const partialTrimmed = partialText.trim();
  if (!partialTrimmed) return finalTrimmed;
  if (!finalTrimmed) return partialTrimmed;
  if (partialTrimmed.startsWith(finalTrimmed)) return partialTrimmed;
  return `${finalTrimmed} ${partialTrimmed}`;
};
