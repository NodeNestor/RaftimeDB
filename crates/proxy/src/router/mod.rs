// Shard router — maps client requests to the correct Raft group.
//
// In Phase 1 (single shard), all requests go to the one Raft group.
// In Phase 2 (multi-shard), this module will:
// - Maintain a shard map (shard_id → Raft group leader)
// - Route clients to the correct shard based on their SpacetimeDB module
// - Handle shard splits and migrations
// - Redirect clients when shards move between nodes

use std::collections::HashMap;

/// Shard identifier.
pub type ShardId = u64;

/// Maps database/module names to shard IDs.
pub struct ShardRouter {
    routes: HashMap<String, ShardId>,
    default_shard: ShardId,
}

impl ShardRouter {
    /// Create a new router. Phase 1: everything goes to shard 0.
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
            default_shard: 0,
        }
    }

    /// Route a request to a shard.
    /// Returns the explicit route if one exists, otherwise the default shard.
    pub fn route(&self, database_name: &str) -> ShardId {
        self.routes
            .get(database_name)
            .copied()
            .unwrap_or(self.default_shard)
    }

    /// Register a database → shard mapping.
    pub fn add_route(&mut self, database_name: String, shard: ShardId) {
        self.routes.insert(database_name, shard);
    }

    /// Remove a database → shard mapping.
    pub fn remove_route(&mut self, database_name: &str) -> Option<ShardId> {
        self.routes.remove(database_name)
    }

    /// Get all registered routes.
    pub fn routes(&self) -> &HashMap<String, ShardId> {
        &self.routes
    }

    /// Number of registered routes.
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Set the default shard for unrouted requests.
    pub fn set_default_shard(&mut self, shard: ShardId) {
        self.default_shard = shard;
    }
}

impl Default for ShardRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_shard_is_zero() {
        let router = ShardRouter::new();
        assert_eq!(router.route("anything"), 0);
    }

    #[test]
    fn test_explicit_route() {
        let mut router = ShardRouter::new();
        router.add_route("game-zone-forest".into(), 1);
        router.add_route("game-zone-desert".into(), 2);

        assert_eq!(router.route("game-zone-forest"), 1);
        assert_eq!(router.route("game-zone-desert"), 2);
        assert_eq!(router.route("unknown-db"), 0); // falls back to default
    }

    #[test]
    fn test_remove_route_falls_back() {
        let mut router = ShardRouter::new();
        router.add_route("my-db".into(), 5);
        assert_eq!(router.route("my-db"), 5);

        let removed = router.remove_route("my-db");
        assert_eq!(removed, Some(5));
        assert_eq!(router.route("my-db"), 0); // back to default
    }

    #[test]
    fn test_custom_default_shard() {
        let mut router = ShardRouter::new();
        router.set_default_shard(42);
        assert_eq!(router.route("anything"), 42);
    }

    #[test]
    fn test_route_count() {
        let mut router = ShardRouter::new();
        assert_eq!(router.route_count(), 0);

        router.add_route("a".into(), 1);
        router.add_route("b".into(), 2);
        assert_eq!(router.route_count(), 2);
    }

    #[test]
    fn test_overwrite_route() {
        let mut router = ShardRouter::new();
        router.add_route("db".into(), 1);
        router.add_route("db".into(), 2);
        assert_eq!(router.route("db"), 2);
        assert_eq!(router.route_count(), 1); // didn't create duplicate
    }

    #[test]
    fn test_remove_nonexistent_route() {
        let mut router = ShardRouter::new();
        assert_eq!(router.remove_route("nope"), None);
    }
}
