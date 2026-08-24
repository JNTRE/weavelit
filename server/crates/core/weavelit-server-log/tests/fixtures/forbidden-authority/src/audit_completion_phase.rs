use weavelit_server_log::{AttemptRecordId, AuditRecordPhase, LogResult};

fn forge_completion_phase(
    attempt_record_id: AttemptRecordId,
    result: LogResult,
) -> AuditRecordPhase {
    AuditRecordPhase::Completion {
        attempt_record_id,
        result,
    }
}

fn main() {}