use weavelit_server_log::AttemptRecordId;

fn clone_attempt_record_identity(attempt_record_id: &AttemptRecordId) {
    let _attempt_record_id: AttemptRecordId = attempt_record_id.clone();
}

fn main() {}