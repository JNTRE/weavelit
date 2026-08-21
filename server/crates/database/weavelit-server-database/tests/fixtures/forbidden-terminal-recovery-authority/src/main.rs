use weavelit_server_database::{
    AuditTerminalAcknowledgementProof, AuditTerminalObligation,
    AuditTerminalObligationIdentifier, AuditTerminalRecoveryPersistence,
    AuditTerminalSupersession, LogConfigurationGeneration, LogConfigurationGenerationKey,
    LogConfigurationGenerationPersistence, LogConfigurationVersion, Name,
    OpaqueAuditTerminalDisposition, OpaqueAuditTerminalProjection, StateIdentifier,
    StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};

fn forge_generation_key() -> LogConfigurationGenerationKey {
    LogConfigurationGenerationKey {
        configuration: StateIdentifier::from_bytes([2; 16]).unwrap(),
        version: LogConfigurationVersion::INITIAL,
    }
}

fn forge_generation(key: LogConfigurationGenerationKey) -> LogConfigurationGeneration {
    LogConfigurationGeneration {
        key,
        module: Name::new("module").unwrap(),
        name: Name::new("configuration").unwrap(),
        enabled: true,
        settings: Box::default(),
        log_types: Box::default(),
    }
}

fn forge_binding() -> StoredAuditDestinationBinding {
    StoredAuditDestinationBinding {
        identifier: [1; 16],
        version: 1,
    }
}

fn forge_obligation(
    identifier: AuditTerminalObligationIdentifier,
    projection: OpaqueAuditTerminalProjection,
    binding: StoredAuditDestinationBinding,
) -> AuditTerminalObligation {
    AuditTerminalObligation {
        identifier,
        projection,
        binding,
    }
}

fn forge_write(obligation: AuditTerminalObligation) -> ValidatedAuditTerminalObligationWrite {
    ValidatedAuditTerminalObligationWrite { obligation }
}

fn forge_acknowledgement(
    identifier: AuditTerminalObligationIdentifier,
    binding: StoredAuditDestinationBinding,
) -> AuditTerminalAcknowledgementProof {
    AuditTerminalAcknowledgementProof {
        identifier,
        binding,
    }
}

fn forge_supersession(
    original_obligation: AuditTerminalObligation,
    disposition: OpaqueAuditTerminalDisposition,
    original_binding: StoredAuditDestinationBinding,
    replacement_binding: StoredAuditDestinationBinding,
    replacement_obligation: ValidatedAuditTerminalObligationWrite,
) -> AuditTerminalSupersession {
    AuditTerminalSupersession {
        original_obligation,
        disposition,
        original_binding,
        replacement_binding,
        replacement_obligation,
    }
}

fn main() {
    let _forged = AuditTerminalRecoveryPersistence { _private: () };
    let _forged = LogConfigurationGenerationPersistence { _private: () };
}