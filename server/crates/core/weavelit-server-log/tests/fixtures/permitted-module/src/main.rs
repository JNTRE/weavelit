use weavelit_server_log::{
    CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
    LogDestinationError, LogDestinationFactory, LogModuleFactoryContext, LogModuleRegistration,
    LogRecordType,
};

struct ExternalDestination;

impl LogDestination for ExternalDestination {
    fn deliver(
        &self,
        record: &CompleteLogRecord,
        acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        let _ = record.persistence_view();
        Ok(acknowledgement)
    }
}

struct ExternalFactory;

impl LogDestinationFactory for ExternalFactory {
    fn create(
        &self,
        context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        let _ = context.local_root();
        let _ = context.deployment_identity();
        Ok(Box::new(ExternalDestination))
    }
}

fn main() {
    let capabilities = LogCapabilities::new(vec![LogRecordType::System])
        .expect("external module capability declaration must be valid");
    let _registration = LogModuleRegistration::new(
        "external-fixture",
        capabilities,
        Box::new(ExternalFactory),
    );
}