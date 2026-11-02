export const DEFAULT_NAME = 'Aether';

export interface AppServerConfig {
  name: string;
  mode: string;
  version: string;
  channel?: string;
  commit?: string;
  build_date?: string;
  release_url?: string | null;
}

let cachedConfig: AppServerConfig | null = null;
let pendingConfigPromise: Promise<AppServerConfig> | null = null;

export async function fetchServerConfig(signal?: AbortSignal): Promise<AppServerConfig> {
  if (cachedConfig) return cachedConfig;
  if (!pendingConfigPromise) {
    pendingConfigPromise = fetch('/api/v1/config', { signal })
      .then(async response => {
        if (!response.ok) {
          return { name: DEFAULT_NAME, mode: 'standalone', version: '0.1.0' };
        }
        const data = (await response.json()) as AppServerConfig;
        cachedConfig = {
          name: data?.name || DEFAULT_NAME,
          mode: data?.mode || 'standalone',
          version: data?.version || '0.1.0',
          channel: data?.channel || 'stable',
          commit: data?.commit || '',
          build_date: data?.build_date || '',
          release_url: data?.release_url || null,
        };
        return cachedConfig;
      })
      .catch(() => ({ name: DEFAULT_NAME, mode: 'standalone', version: '0.1.0' }))
      .finally(() => {
        pendingConfigPromise = null;
      });
  }
  return pendingConfigPromise;
}

export async function fetchBrandName(signal?: AbortSignal): Promise<string> {
  const config = await fetchServerConfig(signal);
  return config.name;
}
