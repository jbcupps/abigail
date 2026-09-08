//! Hive-owned persistence.
//!
//! The embedded SurrealKV store takes an exclusive OS file lock, so exactly one
//! process opens `memory.db`: hive-daemon. Abigail's bounded setup assistant
//! runs inside that owner. Family entity-daemons reach their own scopes through
//! authenticated `POST /v1/persistence/op` instead of opening the file.
//!
//! Scopes are opened lazily and cached: the Hive scope at startup, each
//! `entity_<uuid>` database the first time something addresses it.

use abigail_persistence::{
    EntityScope, PersistenceHandle, PersistenceOp, PersistenceOutcome, PersistenceRequest,
    QueryBinding,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Owns the on-disk store and hands out one cached handle per scope.
pub struct PersistenceRegistry {
    /// Path of the shared store (`{data_root}/memory.db`).
    root: PathBuf,
    /// Cached handles keyed by Surreal database name.
    scopes: Mutex<HashMap<String, PersistenceHandle>>,
}

impl PersistenceRegistry {
    /// Open the store and pin the Hive scope so a failure to take the file lock
    /// surfaces during startup rather than on the first family request.
    pub fn open(root: impl AsRef<Path>) -> anyhow::Result<Arc<Self>> {
        let root = root.as_ref().to_path_buf();
        let registry = Arc::new(Self {
            root,
            scopes: Mutex::new(HashMap::new()),
        });
        registry.handle(&EntityScope::Hive)?;
        tracing::info!(
            "Hive owns the shared persistence store at {}",
            registry.root.display()
        );
        Ok(registry)
    }

    /// Get (or lazily open) the handle for `scope`.
    ///
    /// Opening is done while holding the cache lock. That is deliberate: opening
    /// a scope calls `use_db` and runs schema DDL, and concurrent `use_db` on one
    /// datastore loses to write conflicts (measured: 7 of 8 concurrent callers
    /// failed). Serializing the one-time open is far cheaper than the retry
    /// machinery the alternative would need. Steady-state operations do not take
    /// this lock at all, so Entities stay concurrent once open.
    fn handle(&self, scope: &EntityScope) -> anyhow::Result<PersistenceHandle> {
        let key = scope.database_name();
        let mut scopes = self
            .scopes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());

        if let Some(handle) = scopes.get(&key) {
            return Ok(handle.clone());
        }

        let handle = PersistenceHandle::open_local(&self.root, scope.clone())
            .map_err(|error| anyhow::anyhow!("opening scope {}: {error}", scope.label()))?;
        let handle = handle.restricted()?;
        Ok(scopes.entry(key).or_insert(handle).clone())
    }

    /// Run one operation from a child daemon against the owned store.
    pub fn run(&self, request: PersistenceRequest) -> anyhow::Result<PersistenceOutcome> {
        let scope = EntityScope::from(request.scope);
        validate_scope(&scope)?;
        // The transport carries parameterized data queries, never schema/session
        // management. Reject these keyword tokens even inside comments/strings:
        // conservative rejection is safe because caller values belong in bindings.
        // In particular, a database user plus permissive table permissions is NOT
        // enough to make a caller-supplied USE safe. Pinning scope is mandatory.
        if let PersistenceOp::Execute { sql, .. }
        | PersistenceOp::QueryVec { sql, .. }
        | PersistenceOp::QueryOne { sql, .. } = &request.op
        {
            for token in sql.split(|c: char| !c.is_ascii_alphabetic()) {
                anyhow::ensure!(
                    !["use", "define", "remove", "alter", "rebuild", "option", "live", "kill"]
                        .iter()
                        .any(|forbidden| token.eq_ignore_ascii_case(forbidden)),
                    "Session and schema commands are not allowed in Entity persistence requests"
                );
            }
        }
        let handle = self.handle(&scope)?;

        let outcome = match request.op {
            PersistenceOp::Execute { sql, bindings } => {
                handle.execute(&sql, &to_bindings(bindings))?;
                PersistenceOutcome::Unit
            }
            PersistenceOp::QueryVec { sql, bindings } => PersistenceOutcome::Many {
                rows: handle.query_vec::<serde_json::Value>(&sql, &to_bindings(bindings))?,
            },
            PersistenceOp::QueryOne { sql, bindings } => PersistenceOutcome::Maybe {
                row: handle.query_one::<serde_json::Value>(&sql, &to_bindings(bindings))?,
            },
            PersistenceOp::Upsert { table, id, value } => {
                handle.upsert(&table, &id, &value)?;
                PersistenceOutcome::Unit
            }
            PersistenceOp::Create { table, id, value } => {
                handle.create(&table, &id, &value)?;
                PersistenceOutcome::Unit
            }
            PersistenceOp::Delete { table, id } => {
                handle.delete_record(&table, &id)?;
                PersistenceOutcome::Unit
            }
            PersistenceOp::Select { table, id } => PersistenceOutcome::Maybe {
                row: handle.select_record::<serde_json::Value>(&table, &id)?,
            },
        };

        Ok(outcome)
    }
}

/// Reject scopes the Hive did not mint.
///
/// Entity scopes are Hive-issued UUIDs. Requiring that shape keeps a caller from
/// creating unbounded databases in the family's store, and closes a collision in
/// `EntityScope::database_name`, which maps `-` to `_` — so the free-form ids
/// `a-b` and `a_b` would otherwise address the same database.
fn validate_scope(scope: &EntityScope) -> anyhow::Result<()> {
    let EntityScope::Entity(id) = scope else {
        return Ok(());
    };
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("entity scope must be a UUID, got {id:?}"))
}

fn to_bindings(wire: Vec<abigail_persistence::remote::BindingWire>) -> Vec<QueryBinding> {
    wire.into_iter().map(QueryBinding::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_persistence::ScopeWire;

    const SCOPE_A: &str = "11111111-1111-4111-8111-111111111111";
    const SCOPE_B: &str = "22222222-2222-4222-8222-222222222222";

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("hive_persist_{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn serves_multiple_entity_scopes_from_one_process() {
        let root = temp_root();
        let registry = PersistenceRegistry::open(root.join("memory.db")).unwrap();

        // Two different entities plus the Hive scope, all live at once — the
        // exact shape that used to fail with an OS file lock across processes.
        for (scope, marker) in [
            (ScopeWire::Hive, "hive-row"),
            (
                ScopeWire::Entity {
                    id: SCOPE_A.to_string(),
                },
                "entity-a-row",
            ),
            (
                ScopeWire::Entity {
                    id: SCOPE_B.to_string(),
                },
                "entity-b-row",
            ),
        ] {
            registry
                .run(PersistenceRequest {
                    scope: scope.clone(),
                    op: PersistenceOp::Upsert {
                        table: "hive_meta".to_string(),
                        id: "probe".to_string(),
                        value: serde_json::json!({ "marker": marker }),
                    },
                })
                .unwrap_or_else(|e| panic!("write to {scope:?} failed: {e}"));
        }

        // Each scope must read back its OWN marker — proof the scopes did not
        // bleed into one another through the shared engine session.
        for (scope, expected) in [
            (ScopeWire::Hive, "hive-row"),
            (
                ScopeWire::Entity {
                    id: SCOPE_A.to_string(),
                },
                "entity-a-row",
            ),
            (
                ScopeWire::Entity {
                    id: SCOPE_B.to_string(),
                },
                "entity-b-row",
            ),
        ] {
            let outcome = registry
                .run(PersistenceRequest {
                    scope: scope.clone(),
                    op: PersistenceOp::Select {
                        table: "hive_meta".to_string(),
                        id: "probe".to_string(),
                    },
                })
                .unwrap();
            let PersistenceOutcome::Maybe { row } = outcome else {
                panic!("expected optional row for {scope:?}");
            };
            let row = row.unwrap_or_else(|| panic!("{scope:?} lost its own row"));
            assert_eq!(
                row["marker"], expected,
                "scope {scope:?} read another scope's data"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn entity_scope_must_be_a_uuid() {
        let root = temp_root();
        let registry = PersistenceRegistry::open(root.join("memory.db")).unwrap();

        // `a-b` and `a_b` both normalize to database `entity_a_b`, so free-form
        // ids would let two scopes address one database.
        for bogus in ["a-b", "a_b", "../escape", ""] {
            let result = registry.run(PersistenceRequest {
                scope: ScopeWire::Entity {
                    id: bogus.to_string(),
                },
                op: PersistenceOp::Select {
                    table: "memory_entry".to_string(),
                    id: "x".to_string(),
                },
            });
            assert!(result.is_err(), "scope id {bogus:?} should be rejected");
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The registry caches one handle per scope and reuses it across requests,
    /// so a `USE` statement smuggled into raw SurrealQL must not be able to
    /// repoint that cached handle at another Entity's database for everyone
    /// who comes after.
    #[test]
    fn raw_sql_cannot_repoint_a_cached_scope_handle() {
        let root = temp_root();
        let registry = PersistenceRegistry::open(root.join("memory.db")).unwrap();

        let scope_a = ScopeWire::Entity {
            id: SCOPE_A.to_string(),
        };
        let scope_b = ScopeWire::Entity {
            id: SCOPE_B.to_string(),
        };

        registry
            .run(PersistenceRequest {
                scope: scope_b.clone(),
                op: PersistenceOp::Upsert {
                    table: "memory_entry".to_string(),
                    id: "secret".to_string(),
                    value: serde_json::json!({ "marker": "belongs-to-b" }),
                },
            })
            .unwrap();

        let attack = registry.run(PersistenceRequest {
            scope: scope_a.clone(),
            op: PersistenceOp::QueryVec {
                sql: format!(
                    "USE NS abigail DB entity_{}; SELECT * FROM memory_entry;",
                    SCOPE_B.replace('-', "_")
                ),
                bindings: vec![],
            },
        });
        assert!(
            attack.is_err(),
            "A child must not read another database within one query"
        );

        // Attempt to hijack scope A's cached handle. Succeeding or erroring are
        // both acceptable; silently switching A's session is not.
        let _ = registry.run(PersistenceRequest {
            scope: scope_a.clone(),
            op: PersistenceOp::Execute {
                sql: format!("USE NS abigail DB entity_{};", SCOPE_B.replace('-', "_")),
                bindings: vec![],
            },
        });

        // Scope A must still be scope A: it must not see B's record.
        let outcome = registry
            .run(PersistenceRequest {
                scope: scope_a,
                op: PersistenceOp::Select {
                    table: "memory_entry".to_string(),
                    id: "secret".to_string(),
                },
            })
            .unwrap();
        let PersistenceOutcome::Maybe { row } = outcome else {
            panic!("expected optional row");
        };
        assert!(
            row.is_none(),
            "USE in raw SQL repointed the cached handle: scope A read B's record {row:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Concurrent writes from several Entities are newly possible: before the
    /// Hive owned the store, a second entity-daemon could not even open it. This
    /// checks that simultaneous traffic through the one owned engine succeeds
    /// rather than surfacing retryable transaction conflicts to the family.
    #[test]
    fn concurrent_writes_across_scopes_all_succeed() {
        let root = temp_root();
        let registry = PersistenceRegistry::open(root.join("memory.db")).unwrap();

        let scopes = [
            "33333333-3333-4333-8333-333333333333",
            "44444444-4444-4444-8444-444444444444",
            "55555555-5555-4555-8555-555555555555",
            "66666666-6666-4666-8666-666666666666",
        ];
        let writes_each = 25;

        let failures: Vec<String> = std::thread::scope(|s| {
            let handles: Vec<_> = scopes
                .iter()
                .map(|scope_id| {
                    let registry = &registry;
                    s.spawn(move || {
                        let mut errors = Vec::new();
                        for n in 0..writes_each {
                            let result = registry.run(PersistenceRequest {
                                scope: ScopeWire::Entity {
                                    id: (*scope_id).to_string(),
                                },
                                op: PersistenceOp::Upsert {
                                    table: "memory_entry".to_string(),
                                    id: format!("row-{n}"),
                                    value: serde_json::json!({ "scope": scope_id, "n": n }),
                                },
                            });
                            if let Err(e) = result {
                                errors.push(format!("{scope_id}/{n}: {e}"));
                            }
                        }
                        errors
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("writer thread panicked"))
                .collect()
        });

        assert!(
            failures.is_empty(),
            "{} concurrent write(s) failed, first few: {:?}",
            failures.len(),
            failures.iter().take(5).collect::<Vec<_>>()
        );

        // Every scope must still read back its own last row.
        for scope_id in scopes {
            let outcome = registry
                .run(PersistenceRequest {
                    scope: ScopeWire::Entity {
                        id: scope_id.to_string(),
                    },
                    op: PersistenceOp::Select {
                        table: "memory_entry".to_string(),
                        id: format!("row-{}", writes_each - 1),
                    },
                })
                .unwrap();
            let PersistenceOutcome::Maybe { row } = outcome else {
                panic!("expected optional row");
            };
            let row = row.unwrap_or_else(|| panic!("{scope_id} lost its last row"));
            assert_eq!(
                row["scope"], scope_id,
                "cross-scope bleed under concurrency"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
