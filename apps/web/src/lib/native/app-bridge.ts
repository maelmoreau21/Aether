/**
 * What the Android application adds to the pages it shows (`window.aetherApp`): offline
 * downloads and their list stay native, since a browser cannot keep files for later.
 * Absent in a browser and in the desktop application.
 */
export interface AetherApp {
  download?(id: string, kind: string, title: string): void;
  downloads?(): void;
}

export function nativeApp(): AetherApp | undefined {
  if (typeof window === 'undefined') return undefined;
  return (window as { aetherApp?: AetherApp }).aetherApp;
}
