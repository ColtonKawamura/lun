// lun service worker — cache API responses + static assets
const CACHE = 'lun-v1';
const API_ROUTES = ['/api/projects', '/api/tasks', '/api/scopes', '/api/markdown/preview'];
const STATIC_FILES = ['/', '/manifest.json', '/favicon.ico'];

self.addEventListener('install', event => {
  event.waitUntil(caches.open(CACHE).then(cache => cache.addAll(STATIC_FILES)));
  self.skipWaiting();
});

self.addEventListener('activate', event => {
  event.waitUntil(caches.keys().then(keys =>
    Promise.all(keys.filter(k => k !== CACHE).map(k => caches.delete(k)))
  ));
  self.clients.claim();
});

self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  // Cache static assets
  if (STATIC_FILES.some(p => url.pathname === p)) {
    event.respondWith(caches.match(event.request).then(resp => resp || fetch(event.request)));
    return;
  }
  // API: network-first with cache fallback
  if (API_ROUTES.some(p => url.pathname.startsWith(p))) {
    event.respondWith(
      fetch(event.request).then(resp => {
        const clone = resp.clone();
        caches.open(CACHE).then(cache => cache.put(event.request, clone));
        return resp;
      }).catch(() => caches.match(event.request))
    );
    return;
  }
});
