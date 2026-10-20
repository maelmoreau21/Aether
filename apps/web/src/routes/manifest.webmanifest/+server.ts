import { DEFAULT_NAME } from '../../lib/config/brand';

export const prerender = true;

export function GET() {
  return new Response(JSON.stringify({
    id: '/app/',
    name: DEFAULT_NAME,
    short_name: DEFAULT_NAME,
    lang: 'fr',
    start_url: '/app/',
    scope: '/app/',
    display: 'standalone',
    background_color: '#111827',
    theme_color: '#111827',
    icons: [
      { src: 'icons/app-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' },
      { src: 'icons/app-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' }
    ]
  }), { headers: { 'Content-Type': 'application/manifest+json' } });
}
