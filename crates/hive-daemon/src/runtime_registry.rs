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
    synced_outbox: HashMap<String, Vec<hive_core::EntityOutboxRecord>>,
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
        let request_records = request.records.clone();
        let Some(session) = self.sessions.get_mut(&request.lease_id) else {
            return Err(format!("Unknown runtime lease '{}'", request.lease_id));
        };
        if session.lease.runtime_id != request.runtime_id {
            return Err(format!(
                "Runtime '{}' does not match lease '{}'",
                request.runtime_id, request.lease_id
            ));
        }

        let accepted_record_ids = request
            .records
            .iter()
            .map(|record| record.record_id.clone())
            .collect::<Vec<_>>();
        self.synced_outbox
            .entry(session.lease.entity_id.clone())
            .or_default()
            .extend(request.records);

        session.outbox_depth = 0;
        session.outbox_oldest_at_utc = None;
        let server_time_utc = chrono::Utc::now().to_rfc3339();
        let receipt = if request_records.is_empty() {
            None
        } else {
            let batch_hash = hash_outbox_batch(&request_records);
            let receipt_payload = format!(
                "execution_receipt_v1\nentity_id={}\nruntime_id={}\nlease_id={}\nbatch_hash={}\nevent_count={}\naccepted_record_ids={}",
                session.lease.entity_id,
                request.runtime_id,
                request.lease_id,
                batch_hash,
                accepted_record_ids.len(),
                accepted_record_ids.join(",")
            );
            let (signature, public_key) = signer(receipt_payload.as_bytes());
            Some(ExecutionReceipt {
                schema_version: "execution_receipt_v1".to_string(),
                receipt_id: uuid::Uuid::new_v4().to_string(),
                entity_id: session.lease.entity_id.clone(),
                runtime_id: request.runtime_id,
                lease_id: request.lease_id,
                batch_hash,
                event_count: accepted_record_ids.len(),
                accepted_record_ids: accepted_record_ids.clone(),
                signature,
                public_key,
                signed_at_utc: server_time_utc.clone(),
            })
        };
        let execution_receipts = receipt.into_iter().collect::<Vec<_>>();
        if !execution_receipts.is_empty() {
            self.execution_receipts
                .entry(session.lease.entity_id.clone())
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
}
