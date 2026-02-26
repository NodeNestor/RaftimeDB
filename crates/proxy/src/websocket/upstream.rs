// Upstream SpacetimeDB connection management.
// Currently, each client gets a dedicated upstream connection.
// Future: connection pooling, reconnection, health checks.

// The upstream connection logic is inline in handler.rs for now.
// This module will be expanded for:
// - Automatic reconnection on SpacetimeDB restart
// - Health checking (ping/pong)
// - Connection draining during shard migration
