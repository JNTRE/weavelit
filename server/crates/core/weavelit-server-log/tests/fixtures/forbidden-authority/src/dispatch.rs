use weavelit_server_log::{
    CompleteLogRecord, ConfiguredLogDestination, DurableAcknowledgement, LogCapabilities,
    LogDestination, LogDestinationError, LogRecordType,
};

struct DummyDestination;

impl LogDestination for DummyDestination {
    fn deliver(
        &self,
        _record: &CompleteLogRecord,
        acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        Ok(acknowledgement)
    }

    fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
        Ok(())
    }
}

fn main() {
    let capabilities = LogCapabilities::new(vec![LogRecordType::System]).unwrap();
    let _destination = ConfiguredLogDestination {
        capabilities,
        destination: Box::new(DummyDestination),
    };
}