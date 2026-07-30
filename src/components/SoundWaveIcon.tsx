export function SoundWaveIcon({ size = 16 }: { size?: number }) {
  return (
    <div className="sound-wave">
      <div
        className="sound-wave-bar"
        style={{ width: size * 0.25, height: size * 0.6 }}
      />
      <div
        className="sound-wave-bar"
        style={{ width: size * 0.25, height: size }}
      />
      <div
        className="sound-wave-bar"
        style={{ width: size * 0.25, height: size * 0.6 }}
      />
    </div>
  );
}
