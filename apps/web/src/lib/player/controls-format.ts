/** Formatage et libellés purs de la barre de contrôle du lecteur. */

export type ControlsMenu = 'audio' | 'subtitles' | 'video' | 'speed' | 'settings' | null;
export type AspectRatio = 'contain' | 'cover' | 'fill';

export const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];

/** `mm:ss` ou `h:mm:ss`. */
export function clock(seconds: number): string {
  if (!Number.isFinite(seconds)) return '00:00';
  const hours = Math.floor(seconds / 3600);
  return `${hours ? hours + ':' : ''}${Math.floor((seconds / 60) % 60).toString().padStart(2, '0')}:${Math.floor(seconds % 60).toString().padStart(2, '0')}`;
}

/** Heure de fin projetée (`HHhMM`) ; chaîne vide si inconnue. */
export function projectedEndTime(nowMs: number, remaining: number, duration: number, speed: number): string {
  if (!Number.isFinite(remaining) || remaining <= 0 || !duration) return '';
  const effective = speed > 0 ? speed : 1;
  const end = new Date(nowMs + (remaining / effective) * 1000);
  const hours = end.getHours().toString().padStart(2, '0');
  const minutes = end.getMinutes().toString().padStart(2, '0');
  return `${hours}h${minutes}`;
}

export function menuTitle(menu: ControlsMenu): string {
  return menu === 'audio' ? 'Langue audio'
    : menu === 'subtitles' ? 'Sous-titres'
    : menu === 'video' ? 'Vidéo et sources'
    : menu === 'settings' ? 'Paramètres'
    : 'Vitesse de lecture';
}

export function nextAspectRatio(current: AspectRatio): AspectRatio {
  return current === 'contain' ? 'cover' : current === 'cover' ? 'fill' : 'contain';
}

export function aspectRatioLabel(ratio: AspectRatio): string {
  return ratio === 'cover' ? 'Remplir' : ratio === 'fill' ? 'Étirer' : 'Normal';
}

/** Libellé de vitesse du menu Paramètres (`1x`, `1,5x`). */
export function speedSettingLabel(speed: number): string {
  return speed === 1 ? '1x' : `${speed.toLocaleString('fr')}x`;
}

/** Libellé de vitesse du menu Vitesse (`Normale · 1×`, `1,5×`). */
export function speedOptionLabel(speed: number): string {
  return speed === 1 ? 'Normale · 1×' : `${speed.toLocaleString('fr')}×`;
}

/** Pourcentage (0–100) borné de `value / total`, 0 si `total` est nul. */
export function percentOf(value: number, total: number): number {
  return total > 0 ? Math.min(100, Math.max(0, (value / total) * 100)) : 0;
}
