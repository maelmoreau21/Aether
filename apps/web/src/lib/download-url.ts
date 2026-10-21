// Originals always pass through Core's live download permission, including remote Nodes.
export function downloadUrl(value: string, mime = ''): string | null {
  let url: URL;
  try { url = new URL(value, 'https://media.invalid'); } catch { return null; }
  const own = url.pathname.match(/^\/api\/v1\/(assets\/[^/]+|music\/tracks\/[^/]+)\/content$/);
  if (own) return `/api/v1/${own[1]}/download`;
  const direct = url.pathname.match(/^\/internal\/v1\/node\/assets\/([^/]+)\/content$/);
  if (direct) return `/api/v1/${mime.startsWith('audio/') ? 'music/tracks' : 'assets'}/${direct[1]}/download`;
  return null;
}
