use hive_core::{
    CreateForgeApprovalJobRequest, ExecutionReceipt, ExecutionReceiptsResponse, ForgeApprovalJob,
    ForgeApprovalJobsResponse, OutboxSyncRequest, OutboxSyncResponse, RuntimeHeartbeatRequest,
    RuntimeHeartbeatResponse, RuntimeRegistration, RuntimeRegistrationRequest, RuntimeSessionLease,
    RuntimeSessionRequest, RuntimeSessionStatus, SetSkillAssignmentsRequest,
    SkillAssignmentsResponse,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Default)]
pub struct RuntimeControlPlane {
    sessions: HashMap<String, RuntimeSessionStatus>,
    assignments: HashMap<String, SkillAssignmentsResponse>,
    forge_jobs: HashMap<String, ForgeApprovalJobsResponse>,
    synced_outbox: HashMap<String, HashMap<String, hive_core::EntityOutboxRecord>>,
    execution_receipts: HashMap<String, Vec<ExecutionReceipt>>,
}

impl RuntimeControlPlane {
    pub fn issue_session(
        &mut self,
        request: RuntimeSessionRequest,
        entity_name: Option<String>,
        hive_url: Option<String>,
    ) -> RuntimeSessionLease {
        let runtime_id = request
            .runtime_id
            .unwrap_or_else(|| format!("runtime-{}", uuid::Uuid::new_v4()));
        let lease = RuntimeSessionLease {
            lease_id: uuid::Uuid::new_v4().to_string(),
            entity_id: request.entity_id,
            runtime_id,
            entity_name,
            hive_url,
            issued_at_utc: chrono::Utc::now().to_rfc3339(),
            expires_at_utc: None,
            offline_until_close: true,
            lease_scope: "runtime_session".to_string(),
        };
        self.sessions.insert(
            lease.lease_id.clone(),
            RuntimeSessionStatus {
                lease: lease.clone(),
                registration: None,
                connected: false,
                outbox_depth: 0,
                outbox_oldest_at_utc: None,
            },
        );
        lease
    }

    pub fn register_runtime(
        &mut self,
        request: RuntimeRegistrationRequest,
    ) -> Result<RuntimeSessionStatus, String> {
        let Some(session) = self.sessions.get_mut(&request.lease_id) else {
            return Err(format!("Unknown runtime lease '{}'", request.lease_id));
        };
        if session.lease.runtime_id != request.runtime_id {
            return Err(format!(
                "Runtime '{}' does not match lease '{}'",
                request.runtime_id, request.lease_id
            ));
        }

        let now = chrono::Utc::now().to_rfc3339();
        session.registration = Some(RuntimeRegistration {
            lease_id: request.lease_id,
            runtime_id: request.runtime_id,
            entity_id: session.lease.entity_id.clone(),
            local_url: request.local_url,
            process_id: request.process_id,
            registered_at_utc: now.clone(),
            last_seen_at_utc: now,
            state: "active".to_string(),
        });
        session.connected = true;
        Ok(session.clone())
    }

    pub fn record_heartbeat(
        &mut self,
        request: RuntimeHeartbeatRequest,
    ) -> Result<RuntimeHeartbeatResponse, String> {
        let Some(session) = self.sessions.get_mut(&request.lease_id) else {
            return Err(format!("Unknown runtime lease '{}'", request.lease_id));
        };
        if session.lease.runtime_id != request.runtime_id {
            return Err(format!(
                "Runtime '{}' does not match lease '{}'",
                request.runtime_id, request.lease_id
            ));
        }

        let now = chrono::Utc::now().to_rfc3339();
        if let Some(registration) = session.registration.as_mut() {
            if let Some(local_url) = request.local_url {
                registration.local_url = local_url;
            }
            registration.last_seen_at_utc = now.clone();
            registration.state = "active".to_string();
        }
        session.connected = true;
        session.outbox_depth = request.outbox_depth;
        session.outbox_oldest_at_utc = request.outbox_oldest_at_utc;

        Ok(RuntimeHeartbeatResponse {
            accepted: true,
            server_time_utc: now,
        })
    }

    pub fn session_status(&self, lease_id: &str) -> Option<RuntimeSessionStatus> {
        self.sessions.get(lease_id).cloned()
    }

    /// A family Entity's explicit close ends its local runtime sessions.
    /// Issuing another lease does not revoke existing independent sessions.
    pub fn revoke_entity_sessions(&mut self, entity_id: &str) -> usize {
        let previous_count = self.sessions.len();
        self.sessions
            .retain(|_, session| session.lease.entity_id != entity_id);
        previous_count - self.sessions.len()
    }

    pub fn assignments(&self, entity_id: &str) -> SkillAssignmentsResponse {
        self.assignments
            .get(entity_id)
            .cloned()
            .unwrap_or_else(|| SkillAssignmentsResponse {
                entity_id: entity_id.to_string(),
                assignments: Vec::new(),
            })
    }

    pub fn set_assignments(
        &mut self,
        entity_id: &str,
        request: SetSkillAssignmentsRequest,
    ) -> SkillAssignmentsResponse {
        let response = SkillAssignmentsResponse {
            entity_id: entity_id.to_string(),
            assignments: request.assignments,
        };
        self.assignments
            .insert(entity_id.to_string(), response.clone());
        response
    }

    pub fn forge_jobs(&self, entity_id: &str) -> ForgeApprovalJobsResponse {
        self.forge_jobs
            .get(entity_id)
            .cloned()
            .unwrap_or_else(|| ForgeApprovalJobsResponse {
                entity_id: entity_id.to_string(),
                jobs: Vec::new(),
            })
    }

    pub fn create_forge_job(
        &mut self,
        entity_id: &str,
        request: CreateForgeApprovalJobRequest,
    ) -> ForgeApprovalJob {
        let job = ForgeApprovalJob {
            job_id: uuid::Uuid::new_v4().to_string(),
            entity_id: entity_id.to_string(),
            skill_id: request.skill_id,
            code_path: request.code_path,
            markdown_path: request.markdown_path,
            approved_at_utc: chrono::Utc::now().to_rfc3339(),
            status: "approved".to_string(),
            correlation_id: request.correlation_id,
        };
        let entry = self
            .forge_jobs
            .entry(entity_id.to_string())
            .or_insert_with(|| ForgeApprovalJobsResponse {
                entity_id: entity_id.to_string(),
                jobs: Vec::new(),
            });
        entry.jobs.push(job.clone());
        job
    }

    pub fn sync_outbox<F>(
        &mut self,
        request: OutboxSyncRequest,
        mut signer: F,
    ) -> Result<OutboxSyncResponse, String>
    where
        F: FnMut(&[u8]) -> (String, String),
    {
        let Some(session) = self.sessions.get_mut(&request.lease_id) else {
            return Err(format!("Unknown runtime lease '{}'", request.lease_id));
        };
        if session.lease.runtime_id != request.runtime_id {
            return Err(format!(
                "Runtime '{}' does not match lease '{}'",
                request.runtime_id, request.lease_id
            ));
        }
        if request
            .records
            .iter()
            .any(|record| record.entity_id != session.lease.entity_id)
        {
            return Err("Runtime outbox contains a record for another Entity".to_string());
        }

        let entity_id = session.lease.entity_id.clone();
        let mut accepted_record_ids = Vec::new();
        let mut request_record_indexes = HashMap::<String, usize>::new();
        let mut request_records = Vec::new();
        let saved_records = self.synced_outbox.get(&entity_id);
        for record in request.records {
            if let Some(index) = request_record_indexes.get(&record.record_id) {
                let earlier = &request_records[*index];
                if !same_outbox_record(earlier, &record) {
                    return Err(format!(
                        "Runtime outbox record ID '{}' conflicts within this batch",
                        record.record_id
                    ));
                }
                continue;
            }
            if let Some(saved) = saved_records.and_then(|saved| saved.get(&record.record_id)) {
                if !same_outbox_record(saved, &record) {
                    return Err(format!(
                        "Runtime outbox record ID '{}' conflicts with an accepted record",
                        record.record_id
                    ));
                }
            }
            accepted_record_ids.push(record.record_id.clone());
            request_record_indexes.insert(record.record_id.clone(), request_records.len());
            request_records.push(record);
        }

        let new_records = request_records
            .into_iter()
            .filter(|record| {
                saved_records.is_none_or(|saved| !saved.contains_key(&record.record_id))
            })
            .collect::<Vec<_>>();
        let synced_records = self.synced_outbox.entry(entity_id.clone()).or_default();
        for record in &new_records {
            synced_records.insert(record.record_id.clone(), record.clone());
        }

        session.outbox_depth = 0;
        session.outbox_oldest_at_utc = None;
        let server_time_utc = chrono::Utc::now().to_rfc3339();
        let receipt = if new_records.is_empty() {
            None
        } else {
            let batch_hash = hash_outbox_batch(&new_records);
            let receipt_record_ids = new_records
                .iter()
                .map(|record| record.record_id.clone())
                .collect::<Vec<_>>();
            let receipt_payload = format!(
                "execution_receipt_v1\nentity_id={}\nruntime_id={}\nlease_id={}\nbatch_hash={}\nevent_count={}\naccepted_record_ids={}",
                entity_id,
                request.runtime_id,
                request.lease_id,
                batch_hash,
                receipt_record_ids.len(),
                receipt_record_ids.join(",")
            );
            let (signature, public_key) = signer(receipt_payload.as_bytes());
            Some(ExecutionReceipt {
                schema_version: "execution_receipt_v1".to_string(),
                receipt_id: uuid::Uuid::new_v4().to_string(),
                entity_id: entity_id.clone(),
                runtime_id: request.runtime_id,
                lease_id: request.lease_id,
                batch_hash,
                event_count: receipt_record_ids.len(),
                accepted_record_ids: receipt_record_ids,
                signature,
                public_key,
                signed_at_utc: server_time_utc.clone(),
            })
        };
        let execution_receipts = receipt.into_iter().collect::<Vec<_>>();
        if !execution_receipts.is_empty() {
            self.execution_receipts
                .entry(entity_id)
                .or_default()
                .extend(execution_receipts.clone());
        }

        Ok(OutboxSyncResponse {
            accepted_record_ids,
            pending_records: 0,
            server_time_utc,
            execution_receipts,
        })
    }

    pub fn execution_receipts(&self, entity_id: &str, limit: usize) -> ExecutionReceiptsResponse {
        let mut receipts = self
            .execution_receipts
            .get(entity_id)
            .cloned()
            .unwrap_or_default();
        if receipts.len() > limit {
            receipts = receipts.split_off(receipts.len() - limit);
        }
        ExecutionReceiptsResponse {
            entity_id: entity_id.to_string(),
            receipts,
        }
    }
}

fn same_outbox_record(
    left: &hive_core::EntityOutboxRecord,
    right: &hive_core::EntityOutboxRecord,
) -> bool {
    left.record_id == right.record_id
        && left.entity_id == right.entity_id
        && left.kind == right.kind
        && left.created_at_utc == right.created_at_utc
        && left.payload == right.payload
}

fn hash_outbox_batch(records: &[hive_core::EntityOutboxRecord]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"execution_receipt_v1\n");
    for record in records {
        hasher.update(record.record_id.as_bytes());
        hasher.update(b"\n");
        hasher.update(record.entity_id.as_bytes());
        hasher.update(b"\n");
        hasher.update(record.kind.as_bytes());
        hasher.update(b"\n");
        hasher.update(record.created_at_utc.as_bytes());
        hasher.update(b"\n");
        let payload = serde_json::to_vec(&record.payload).unwrap_or_default();
        hasher.update(Sha256::digest(payload));
        hasher.update(b"\n");
    }
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::RuntimeControlPlane;
    use hive_core::{
        OutboxSyncRequest, RuntimeHeartbeatRequest, RuntimeRegistrationRequest,
        RuntimeSessionRequest, SkillAssignment,
    };

    #[test]
    fn runtime_session_registers_and_accepts_outbox_sync() {
        let mut control = RuntimeControlPlane::default();
        let lease = control.issue_session(
            RuntimeSessionRequest {
                entity_id: "entity-1".to_string(),
                runtime_id: Some("runtime-1".to_string()),
            },
            Some("Entity One".to_string()),
            Some("http://127.0.0.1:3141".to_string()),
        );

        let status = control
            .register_runtime(RuntimeRegistrationRequest {
                lease_id: lease.lease_id.clone(),
                runtime_id: lease.runtime_id.clone(),
                local_url: "http://127.0.0.1:9001".to_string(),
                process_id: Some(42),
            })
            .unwrap();
        assert!(status.connected);

        let heartbeat = control
            .record_heartbeat(RuntimeHeartbeatRequest {
                lease_id: lease.lease_id.clone(),
                runtime_id: lease.runtime_id.clone(),
                local_url: None,
                outbox_depth: 2,
                outbox_oldest_at_utc: Some("2026-04-12T12:00:00Z".to_string()),
            })
            .unwrap();
        assert!(heartbeat.accepted);

        let sync = control
            .sync_outbox(
                OutboxSyncRequest {
                    lease_id: lease.lease_id.clone(),
                    runtime_id: lease.runtime_id.clone(),
                    records: vec![hive_core::EntityOutboxRecord {
                        record_id: "record-1".to_string(),
                        entity_id: "entity-1".to_string(),
                        kind: "chat_user_turn".to_string(),
                        created_at_utc: "2026-04-12T12:00:00Z".to_string(),
                        payload: serde_json::json!({ "message": "hello" }),
                    }],
                },
                |_| ("signature".to_string(), "public-key".to_string()),
            )
            .unwrap();
        assert_eq!(sync.accepted_record_ids, vec!["record-1".to_string()]);
        assert_eq!(sync.execution_receipts.len(), 1);
    }

    #[test]
    fn assignments_replace_existing_set() {
        let mut control = RuntimeControlPlane::default();
        let response = control.set_assignments(
            "entity-1",
            hive_core::SetSkillAssignmentsRequest {
                assignments: vec![SkillAssignment {
                    assignment_id: "assign-1".to_string(),
                    entity_id: "entity-1".to_string(),
                    skill_id: "dynamic.calendar".to_string(),
                    version: Some("1.0.0".to_string()),
                    manifest_path: None,
                    assigned_at_utc: "2026-04-12T12:00:00Z".to_string(),
                    status: "assigned".to_string(),
                }],
            },
        );
        assert_eq!(response.assignments.len(), 1);
        assert_eq!(control.assignments("entity-1").assignments.len(), 1);
    }

    #[test]
    fn outbox_sync_rejects_records_owned_by_another_entity() {
        let mut control = RuntimeControlPlane::default();
        let lease = control.issue_session(
            RuntimeSessionRequest {
                entity_id: "entity-1".to_string(),
                runtime_id: Some("runtime-1".to_string()),
            },
            None,
            None,
        );
        let result = control.sync_outbox(
            OutboxSyncRequest {
                lease_id: lease.lease_id,
                runtime_id: lease.runtime_id,
                records: vec![hive_core::EntityOutboxRecord {
                    record_id: uuid::Uuid::new_v4().to_string(),
                    entity_id: "entity-2".to_string(),
                    kind: "skill_used".to_string(),
                    created_at_utc: "2026-10-22T12:00:00Z".to_string(),
                    payload: serde_json::json!({
                        "session_id": "session-1",
                        "skill_id": "com.abigail.calendar",
                        "tool_name": "list_events",
                    }),
                }],
            },
            |_| ("signature".to_string(), "public-key".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn outbox_sync_acknowledges_replays_without_duplicate_records_or_receipts() {
        let mut control = RuntimeControlPlane::default();
        let lease = control.issue_session(
            RuntimeSessionRequest {
                entity_id: "entity-1".to_string(),
                runtime_id: Some("runtime-1".to_string()),
            },
            None,
            None,
        );
        let record = hive_core::EntityOutboxRecord {
            record_id: uuid::Uuid::new_v4().to_string(),
            entity_id: "entity-1".to_string(),
            kind: "skill_used".to_string(),
            created_at_utc: "2026-10-22T12:00:00Z".to_string(),
            payload: serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        };
        let request = || OutboxSyncRequest {
            lease_id: lease.lease_id.clone(),
            runtime_id: lease.runtime_id.clone(),
            records: vec![record.clone()],
        };

        let first = control
            .sync_outbox(request(), |_| {
                ("signature".to_string(), "public-key".to_string())
            })
            .unwrap();
        let replay = control
            .sync_outbox(request(), |_| {
                ("signature".to_string(), "public-key".to_string())
            })
            .unwrap();

        assert_eq!(first.accepted_record_ids, vec![record.record_id.clone()]);
        assert_eq!(replay.accepted_record_ids, vec![record.record_id.clone()]);
        assert_eq!(first.execution_receipts.len(), 1);
        assert!(
            replay.execution_receipts.is_empty(),
            "an accepted replay must not mint a duplicate receipt"
        );
        assert_eq!(control.synced_outbox["entity-1"].len(), 1);
        assert_eq!(
            control.execution_receipts("entity-1", 500).receipts.len(),
            1
        );
    }

    #[test]
    fn outbox_sync_prevalidates_record_id_collisions_before_mutating_state() {
        let mut control = RuntimeControlPlane::default();
        let lease = control.issue_session(
            RuntimeSessionRequest {
                entity_id: "entity-1".to_string(),
                runtime_id: Some("runtime-1".to_string()),
            },
            None,
            None,
        );
        let record_id = uuid::Uuid::new_v4().to_string();
        let record = |tool_name: &str| hive_core::EntityOutboxRecord {
            record_id: record_id.clone(),
            entity_id: "entity-1".to_string(),
            kind: "skill_used".to_string(),
            created_at_utc: "2026-10-22T12:00:00Z".to_string(),
            payload: serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": tool_name,
            }),
        };
        let request = |records| OutboxSyncRequest {
            lease_id: lease.lease_id.clone(),
            runtime_id: lease.runtime_id.clone(),
            records,
        };

        control
            .sync_outbox(request(vec![record("list_events")]), |_| {
                ("signature".to_string(), "public-key".to_string())
            })
            .unwrap();
        let error = control
            .sync_outbox(request(vec![record("delete_event")]), |_| {
                ("signature".to_string(), "public-key".to_string())
            })
            .unwrap_err();

        assert!(error.contains("conflicts"), "{error}");
        assert_eq!(control.synced_outbox["entity-1"].len(), 1);
        assert_eq!(
            control.execution_receipts("entity-1", 500).receipts.len(),
            1
        );
    }
}
