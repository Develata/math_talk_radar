//! Schema migrations (§65). Must be transactional; failures leave no
//! half-migrated state. Destructive migrations must be explicit. `first_seen`
//! and media history must not be silently lost. Implementation lands in M3.
