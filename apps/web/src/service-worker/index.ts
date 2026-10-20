import { self } from '$app/service-worker';

// Network only: never persist sessions, catalogue, artwork or media responses.
// No forced activation during playback. Navigations can retry once online.
self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'GET' || event.request.mode !== 'navigate' ||
      url.origin !== self.location.origin || !url.pathname.startsWith('/app/')) return;
  event.respondWith(fetch(event.request).catch(() => new Response(
    '<!doctype html><html lang="fr"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Connexion indisponible</title><h1>Connexion indisponible</h1><p>Vérifiez votre connexion et la disponibilité du serveur.</p><p>La lecture des médias nécessite une connexion au serveur.</p><a href="/app/">Réessayer</a></html>',
    { status: 503, headers: { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' } }
  )));
});
