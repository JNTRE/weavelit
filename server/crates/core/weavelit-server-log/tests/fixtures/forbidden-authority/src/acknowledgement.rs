use weavelit_server_log::{CompleteLogRecord, DurableAcknowledgement};

fn forge_acknowledgement(record: &CompleteLogRecord) {
    let _acknowledgement = DurableAcknowledgement::for_record(record);
}

fn main() {}