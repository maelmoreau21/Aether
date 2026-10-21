// Only Core-controlled local image endpoints support bounded representations.
export function imagePreviewUrl(url: string | null | undefined, width: 320 | 640 | 1280): string {
  if (!url) return '';
  const [path, query = ''] = url.split('?');
  if (!(path.startsWith('/api/v1/assets/') && path.endsWith('/image')) && !/^\/api\/v1\/works\/[^/]+\/(images\/[^/]+|poster)$/.test(path)) return url;
  const parameters = new URLSearchParams(query);
  parameters.set('width',String(width));
  return `${path}?${parameters}`;
}
