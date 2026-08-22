use weavelit_server_log::{AttemptRecordId, AuditRecordPhase, LogResult};

fn forge_correction_phase(
    attempt_record_id: AttemptRecordId,
    result: LogResult,
) -> AuditRecordPhase {
    AuditRecordPhase::Correction {
        attempt_record_id,
        result,
    }
}

fn main() {}