/** Readable client identity from a session's device label (User-Agent or app label). */
export type ClientKind = 'apple' | 'android' | 'windows' | 'linux' | 'tv' | 'browser';
export type ClientInfo = { app: string; system: string; kind: ClientKind };

export function clientInfo(raw: string | null | undefined): ClientInfo {
  const label = (raw ?? '').trim();
  const system = /iPad/i.test(label) ? 'iPad' : /iPhone|iOS/i.test(label) ? 'iPhone'
    : /Android TV|Google TV|AFT|BRAVIA|SmartTV|Tizen|webOS|tvOS|Apple ?TV|Roku/i.test(label) ? 'Téléviseur'
    : /Android/i.test(label) ? 'Android' : /Windows/i.test(label) ? 'Windows'
    : /Macintosh|Mac OS X|macOS/i.test(label) ? 'Mac' : /CrOS/i.test(label) ? 'ChromeOS' : /Linux/i.test(label) ? 'Linux' : '';
  const kind: ClientKind = system === 'Téléviseur' ? 'tv' : /iPad|iPhone|Mac/.test(system) ? 'apple'
    : system === 'Android' ? 'android' : system === 'Windows' ? 'windows' : system === 'Linux' ? 'linux' : 'browser';
  if (!label.startsWith('Mozilla/')) {
    // Native clients send their own label ("Swiftfin iOS 1.6.1", "Aether TV"…).
    return { app: label.slice(0, 80) || 'Appareil non communiqué', system, kind };
  }
  const browser = /Edg\//.test(label) ? 'Edge' : /OPR\//.test(label) ? 'Opera' : /Firefox\/|FxiOS/.test(label) ? 'Firefox'
    : /Chrome\/|CriOS/.test(label) ? 'Chrome' : /Safari\//.test(label) ? 'Safari' : 'Navigateur';
  return { app: browser, system, kind };
}

/** h:mm:ss under a day, m:ss under an hour. */
export function clock(seconds: number | null | undefined): string {
  const value = Math.max(0, Math.floor(seconds ?? 0));
  const h = Math.floor(value / 3600), m = Math.floor((value % 3600) / 60), s = value % 60;
  return h ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}` : `${m}:${String(s).padStart(2, '0')}`;
}
