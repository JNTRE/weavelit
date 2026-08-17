//! Transformation of a validated backup into replacement application state.
//!
//! Every secret recovered from the backup is re-sealed under the replacement
//! deployment's own at-rest key before it reaches the Application Database. The
//! backup's own protection is never carried forward, and no plaintext secret is
//! retained beyond the transformation.

use weavelit_server_database::{
    ApplicationState, ApplicationStateInput, CompletionObligation, MfaFactor, ProtectedSecret,
    ProtectedValue, ServiceConnection,
};
use weavelit_server_lifecycle::{ProtectedValueKind, ProtectedValueSealer};

use crate::{RestoreError, SensitiveBytes, ValidatedBackup};

/// Re-seals every recovered secret and assembles the replacement state.
///
/// The caller supplies the completion obligation so the committed state and the
/// System Log record it obliges are built from the same fields.
///
/// # Errors
///
/// Returns [`RestoreError::RestoreFailed`] when a secret cannot be sealed or the
/// assembled state is not internally consistent.
pub fn build_application_state(
    validated: &ValidatedBackup,
    sealer: &dyn ProtectedValueSealer,
    completion_obligation: CompletionObligation,
) -> Result<ApplicationState, RestoreError> {
    let backup = validated.backup();

    let protected_secrets = backup
        .protected_secrets()
        .iter()
        .map(|secret| {
            Ok(ProtectedSecret {
                component: secret.component.clone(),
                key: secret.key.clone(),
                value: seal(sealer, ProtectedValueKind::ComponentSecret, &secret.value)?,
            })
        })
        .collect::<Result<Vec<_>, RestoreError>>()?;

    let mfa_factors = backup
        .mfa_factors()
        .iter()
        .map(|factor| {
            Ok(MfaFactor {
                identifier: factor.identifier,
                account: factor.account,
                module: factor.module.clone(),
                protected_factor_data: seal(
                    sealer,
                    ProtectedValueKind::MfaFactorData,
                    &factor.factor_data,
                )?,
            })
        })
        .collect::<Result<Vec<_>, RestoreError>>()?;

    let service_connections = backup
        .service_connections()
        .iter()
        .map(|connection| {
            Ok(ServiceConnection {
                identifier: connection.identifier,
                service_module: connection.service_module.clone(),
                name: connection.name.clone(),
                protected_credential: seal(
                    sealer,
                    ProtectedValueKind::ServiceConnectionCredential,
                    &connection.credential,
                )?,
            })
        })
        .collect::<Result<Vec<_>, RestoreError>>()?;

    ApplicationState::new(ApplicationStateInput {
        configuration: backup.configuration().to_vec(),
        protected_secrets,
        accounts: backup.accounts().to_vec(),
        password_verifiers: backup.password_verifiers().to_vec(),
        groups: backup.groups().to_vec(),
        group_memberships: backup.group_memberships().to_vec(),
        group_grants: backup.group_grants().to_vec(),
        mfa_factors,
        service_connections,
        recovery_public_key: backup.recovery_public_key().clone(),
        log_module_configurations: backup.log_module_configurations().to_vec(),
        log_assignments: backup.log_assignments().to_vec(),
        completion_obligation,
    })
    .map_err(|_| RestoreError::RestoreFailed)
}

fn seal(
    sealer: &dyn ProtectedValueSealer,
    kind: ProtectedValueKind,
    plaintext: &SensitiveBytes,
) -> Result<ProtectedValue, RestoreError> {
    sealer
        .seal(kind, plaintext.expose())
        .map_err(|_| RestoreError::RestoreFailed)
}
