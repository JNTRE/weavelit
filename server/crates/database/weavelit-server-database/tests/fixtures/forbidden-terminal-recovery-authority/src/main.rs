use weavelit_server_database::{
    AuditTerminalObligation, AuditTerminalObligationIdentifier,
    AuditTerminalRecoveryPersistence,
};

fn forge_obligation(identifier: AuditTerminalObligationIdentifier) -> AuditTerminalObligation {
    AuditTerminalObligation {
        identifier,
        projection: b"forged-projection".to_vec().into_boxed_slice(),
    }
}

fn main() {
    let _forged = AuditTerminalRecoveryPersistence { _private: () };
}