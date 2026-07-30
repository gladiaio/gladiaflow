export function VoiceWaveIndicator({ isListening }: { isListening: boolean }) {
  return (
    <div className="voice-wave">
      {[...Array(4)].map((_, i) => (
        <div
          key={i}
          className={`voice-wave-bar ${isListening ? "active" : ""}`}
          style={{ height: isListening ? undefined : 8 }}
        />
      ))}
    </div>
  );
}
