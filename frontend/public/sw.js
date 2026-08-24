self.addEventListener('push', (event) => {
  const data = event.data ? event.data.json() : {};
  event.waitUntil(
    self.registration.showNotification(data.title || 'DevRail 通知', {
      body: data.summary || '有新的 DevRail 事件',
      data: { deepLink: data.deepLink || '/devrail/notifications' },
      tag: data.notificationId ? `devrail-${data.notificationId}` : 'devrail-notification',
    }),
  );
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  event.waitUntil(
    self.clients.openWindow(event.notification.data?.deepLink || '/devrail/notifications'),
  );
});
