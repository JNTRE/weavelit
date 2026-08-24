use weavelit_server_log::{
    AuditLogBody, AuditRecordPhase, CompleteLogRecord, CorrelationId, EventTime, RecordId,
};

fn forge_audit_record(
    record_id: RecordId,
    event_time: EventTime,
    phase: AuditRecordPhase,
    correlation_id: CorrelationId,
    body: AuditLogBody,
) -> CompleteLogRecord {
    CompleteLogRecord::Audit {
        record_id,
        event_time,
        phase,
        correlation_id,
        body,
    }
}

fn main() {}