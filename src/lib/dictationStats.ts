export type DictationStats = {
  totalWords: number;
  totalSeconds: number;
};

export const DEFAULT_DICTATION_STATS: DictationStats = {
  totalWords: 0,
  totalSeconds: 0,
};

export const DICTATION_STATS_STORAGE_KEY = "gladiaflow.dictation.stats.v1";

export const getDictationComment = (totalSeconds: number): string => {
  const totalMinutes = totalSeconds / 60;
  if (totalMinutes < 5) return "Warming up the vocal cords. Keep going.";
  if (totalMinutes < 30) return "Nice pace. Your keyboard is getting jealous.";
  if (totalMinutes < 120) return "You definitely really like to talk.";
  if (totalMinutes < 300)
    return "At this point your voice has a gym membership.";
  return "Legendary mic endurance unlocked.";
};
