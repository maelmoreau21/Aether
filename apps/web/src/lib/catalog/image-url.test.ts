import { expect, test } from 'vitest';
import { imagePreviewUrl, isImageLoaded, markImageLoaded } from './image-url';

test('preview sizes preserve the scanned revision and leave other URLs alone', () => {
  expect(imagePreviewUrl('/api/v1/assets/image-id/image?v=123-456',320)).toBe('/api/v1/assets/image-id/image?v=123-456&width=320');
  expect(imagePreviewUrl('/api/v1/assets/id/image?width=640&v=abc',1280)).toBe('/api/v1/assets/id/image?width=1280&v=abc');
  expect(imagePreviewUrl('https://example.com/poster.png',320)).toBe('https://example.com/poster.png');
  expect(imagePreviewUrl(null,320)).toBe('');
});

test('tracks loaded image URLs in memory for instant subsequent display', () => {
  expect(isImageLoaded('/api/v1/assets/test/image')).toBe(false);
  markImageLoaded('/api/v1/assets/test/image');
  expect(isImageLoaded('/api/v1/assets/test/image')).toBe(true);
  expect(isImageLoaded(null)).toBe(false);
});
