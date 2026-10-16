export const DEFAULT_NAME = 'Aether';

let cachedName: string | null = null;
let pendingPromise: Promise<string> | null = null;

export async function fetchBrandName(signal?: AbortSignal): Promise<string> {
  if (cachedName) return cachedName;
  if (!pendingPromise) {
    pendingPromise = fetch('/api/v1/config', { signal })
      .then(async response => {
        if (!response.ok) return DEFAULT_NAME;
        const config = (await response.json()) as { name?: string };
        cachedName = config?.name || DEFAULT_NAME;
        return cachedName;
      })
      .catch(() => DEFAULT_NAME)
      .finally(() => {
        pendingPromise = null;
      });
  }
  return pendingPromise;
}
