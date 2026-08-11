use weavelit_server_log::{
    CompleteLogRecord, DurableAcknowledgement, LogCapabilities, LogDestination,
    LogDestinationError, LogDestinationFactory, LogModuleCatalog, LogModuleFactoryContext,
    LogModuleIdentifier, LogModuleRegistration, LogRecordType,
};

struct NestedDestination;

impl LogDestination for NestedDestination {
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

struct NestedFactory;

impl LogDestinationFactory for NestedFactory {
    fn create(
        &self,
        context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        let capabilities = LogCapabilities::new(vec![LogRecordType::System]).unwrap();
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "nested",
            capabilities,
            Box::new(NestedFactory),
        )])
        .unwrap();
        let identifier = LogModuleIdentifier::new("nested").unwrap();
        let _destination = catalog.create_destination(&identifier, context);

        Ok(Box::new(NestedDestination))
    }
}

fn main() {}