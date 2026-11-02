/** Remote provider previews use the authenticated, host-restricted Core route. */
export function providerImagePreview(provider: string, workId: string, url: string): string {
  if (url.startsWith('/') && !url.startsWith('//')) return url;
  return `/api/v1/admin/extensions/${encodeURIComponent(provider)}/image-preview?${new URLSearchParams({ workId, url })}`;
}
