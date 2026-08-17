use weavelit_server_log::{
    CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
    LogDestinationError, LogDestinationFactory, LogModuleFactoryContext, LogModuleRegistration,
    LogRecordType, LogSettingsContract,
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

    fn preflight(&self, record_type: LogRecordType) -> Result<(), LogDestinationError> {
        let _ = record_type;
        Ok(())
    }
}

struct ExternalFactory;

impl LogDestinationFactory for ExternalFactory {
    fn accepted_settings(&self) -> LogSettingsContract {
        LogSettingsContract::new(vec!["retention-days".to_owned()])
            .expect("external module settings declaration must be valid")
    }

    fn create(
        &self,
        context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        let _ = context.local_root();
        let _ = context.deployment_identity();
        let _ = context.settings().len();
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