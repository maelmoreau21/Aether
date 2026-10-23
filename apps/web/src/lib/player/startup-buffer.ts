// Let canplay start with the first decoded frames. A large reserve while paused
// can delay startup or time out even though the browser already has playable data.
export function bufferedAhead(media: HTMLMediaElement): number {
  const position = media.currentTime;
  for (let i = 0; i < media.buffered.length; i++) {
    if (media.buffered.start(i) <= position + .025 && media.buffered.end(i) > position) {
      return media.buffered.end(i) - position;
    }
  }
  return 0;
}

export function waitForStartupBuffer(media: HTMLMediaElement, signal: AbortSignal): Promise<void> {
  const enough = () => {
    if (media.seeking || media.readyState < 3) return false;
    const remaining = Number.isFinite(media.duration) ? Math.max(0, media.duration - media.currentTime) : Infinity;
    return bufferedAhead(media) >= Math.min(.05, Math.max(0, remaining - .05));
  };
  if (signal.aborted) return Promise.reject(new Error('Démarrage annulé.'));
  if (enough()) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const events = ['progress', 'canplay', 'canplaythrough', 'loadeddata', 'seeked', 'durationchange'];
    const finish = (error?: Error) => {
      clearTimeout(timeout); clearInterval(poll);
      for (const event of events) media.removeEventListener(event, check);
      media.removeEventListener('error', failed); signal.removeEventListener('abort', cancelled);
      if (error) reject(error); else resolve();
    };
    const check = () => { if (enough()) finish(); };
    const failed = () => finish(new Error('Le flux ne peut pas être mis en mémoire tampon.'));
    const cancelled = () => finish(new Error('Démarrage annulé.'));
    const timeout = setTimeout(() => finish(new Error('Réserve de lecture insuffisante après 8 secondes. Vérifiez le débit du stockage ou du réseau.')), 8000);
    const poll = setInterval(check, 100);
    for (const event of events) media.addEventListener(event, check);
    media.addEventListener('error', failed); signal.addEventListener('abort', cancelled, {once:true});
    check();
  });
}
