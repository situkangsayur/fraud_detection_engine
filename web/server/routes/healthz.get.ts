// Liveness for the compose healthcheck — deliberately does not call the gateway (web must stay up to show a
// "platform unavailable" page when backends are down).
export default defineEventHandler(() => ({ status: 'ok' }))
