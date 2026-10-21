import { expect, test } from 'vitest';
import { imagePreviewUrl } from './image-url';
test('preview sizes preserve the scanned revision and leave other URLs alone', () => {
  expect(imagePreviewUrl('/api/v1/assets/image-id/image?v=123-456',320)).toBe('/api/v1/assets/image-id/image?v=123-456&width=320');
  expect(imagePreviewUrl('/api/v1/assets/id/image?width=640&v=abc',1280)).toBe('/api/v1/assets/id/image?width=1280&v=abc');
  expect(imagePreviewUrl('https://example.com/poster.png',320)).toBe('https://example.com/poster.png');
  expect(imagePreviewUrl(null,320)).toBe('');
});
