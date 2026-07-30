const FUNNY_HOME_MESSAGES: string[] = [
  "Your mic misses you already.",
  "Ready for your next brilliant voice note?",
  "Keyboard break in progress. Talk to me.",
  "Another dictation? Your words are waiting.",
  "Say it out loud, I type the magic.",
  "Voice mode is warmed up and caffeinated.",
  "Let's rest your fingers for a while.",
];

export const pickFunnyHomeMessage = (previous?: string): string => {
  if (FUNNY_HOME_MESSAGES.length === 0) return "";
  if (FUNNY_HOME_MESSAGES.length === 1) return FUNNY_HOME_MESSAGES[0];
  let next =
    FUNNY_HOME_MESSAGES[Math.floor(Math.random() * FUNNY_HOME_MESSAGES.length)];
  while (next === previous) {
    next =
      FUNNY_HOME_MESSAGES[
        Math.floor(Math.random() * FUNNY_HOME_MESSAGES.length)
      ];
  }
  return next;
};
