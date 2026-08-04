use weavelit_server_log::{CompleteLogRecord, RecordId};

fn forge_record_identity(record: &CompleteLogRecord) {
    let _record_id: RecordId = record.record_id().clone();
}

fn main() {}