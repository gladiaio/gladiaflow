class AudioCue {
  private ctx: AudioContext | null = null;

  init() {
    try {
      this.ctx = new (
        window.AudioContext || (window as any).webkitAudioContext
      )();
    } catch (e) {
      console.warn("Web Audio API not available");
    }
  }

  private getOrCreateContext() {
    if (!this.ctx) {
      this.init();
    }
    return this.ctx;
  }

  playStartSound() {
    this.playTone(440, 0.1, 0.05);
  }

  playStopSound() {
    this.playTone(330, 0.15, 0.05);
  }

  playErrorSound() {
    this.playTone(220, 0.2, 0.1);
  }

  private playTone(freq: number, duration: number, volume: number) {
    const ctx = this.getOrCreateContext();
    if (!ctx) return;

    const scheduleTone = () => {
      const oscillator = ctx.createOscillator();
      const gainNode = ctx.createGain();

      oscillator.connect(gainNode);
      gainNode.connect(ctx.destination);

      oscillator.frequency.value = freq;
      oscillator.type = "sine";

      gainNode.gain.setValueAtTime(volume, ctx.currentTime);
      gainNode.gain.exponentialRampToValueAtTime(
        0.01,
        ctx.currentTime + duration,
      );

      oscillator.start(ctx.currentTime);
      oscillator.stop(ctx.currentTime + duration);
    };

    if (ctx.state === "suspended") {
      ctx
        .resume()
        .then(scheduleTone)
        .catch(() => {});
      return;
    }

    scheduleTone();
  }
}

export const audioCue = new AudioCue();
