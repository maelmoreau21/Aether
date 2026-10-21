import { describe, it, expect } from 'vitest';
import { downloadUrl } from './download-url';
describe('download permission route', () => {
  it('routes original and remote Node downloads through Core without bearer tokens', () => {
    expect(downloadUrl('/api/v1/assets/item/content?token=private')).toBe('/api/v1/assets/item/download');
    expect(downloadUrl('https://node.example/internal/v1/node/assets/item/content?token=private','video/mp4')).toBe('/api/v1/assets/item/download');
    expect(downloadUrl('https://node.example/internal/v1/node/assets/track/content?token=private','audio/opus')).toBe('/api/v1/music/tracks/track/download');
    expect(downloadUrl('/api/v1/music/tracks/track/content')).toBe('/api/v1/music/tracks/track/download');
  });
  it('rejects prepared components and arbitrary URLs', () => {
    for (const value of ['/api/v1/assets/x/prepared/video','https://evil.example/film.mp4','javascript:alert(1)','']) expect(downloadUrl(value)).toBeNull();
  });
});
