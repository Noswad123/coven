'use strict';

// No fetch handler or offline cache: private content is never persisted here.
self.addEventListener('push', event => {
  let payload;
  try {
    const text = event.data?.text();
    if (typeof text !== 'string' || new TextEncoder().encode(text).length > 256) return;
    // This closed flat format also refuses duplicate keys and escaped selectors.
    if (!/^\s*\{\s*(?:"version"\s*:\s*1\s*,\s*"alert_id"\s*:\s*"[a-f0-9]{64}"|"alert_id"\s*:\s*"[a-f0-9]{64}"\s*,\s*"version"\s*:\s*1)\s*\}\s*$/.test(text)) return;
    payload = JSON.parse(text);
  } catch { return; }
  event.waitUntil(self.registration.showNotification('Coven needs your attention', {
    body: 'An agent is waiting for your response', tag: payload.alert_id,
    data: {alert_id: payload.alert_id},
  }));
});

self.addEventListener('notificationclick', event => {
  event.notification.close();
  const id = event.notification.data?.alert_id;
  if (event.action || typeof id !== 'string' || !/^[a-f0-9]{64}$/.test(id)) return;
  const url = new URL(`/#attention=${id}`, self.location.origin).href;
  event.waitUntil((async () => {
    const windows = await self.clients.matchAll({type: 'window', includeUncontrolled: true});
    for (const client of windows) {
      if (new URL(client.url).origin !== self.location.origin) continue;
      const navigated = await client.navigate(url);
      if (navigated) { await navigated.focus(); return; }
    }
    await self.clients.openWindow(url);
  })());
});
