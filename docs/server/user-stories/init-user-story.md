# Init User Story

This document defines the user-visible first-launch **[Init](../../glossary.md#states-and-requests)**
story for the **[Web UI](../../glossary.md#applications-and-interfaces)**. It owns
the setup sequence, user responsibilities, visible transitions, and interrupted
workflow behavior. The [Server Init Design](../lifecycle/init/init-design.md) remains authoritative
for new-state requests, recovery-key delivery, and initial-state creation. The
[Server Lifecycle Design](../lifecycle/lifecycle-design.md) owns shared status, database
selection, lifecycle enforcement, and sealing. Another Init-capable client may
present a different interaction while invoking the same Server-owned contracts.

## Install And Open Setup

1. A person installs and starts the
   **[Weavelit Server](../../glossary.md#applications-and-interfaces)** package.
   Package configuration supplies the HTTPS listener, TLS material, and
   protected Server state directory.
2. On first startup, the Server creates its deployment record with a unique
   deployment identifier and `Uninitialized` lifecycle state.
3. The Server enters restricted pre-operational mode. It serves only declared
   **[Init](../../glossary.md#states-and-requests)** and
   **[Restore](../../glossary.md#states-and-requests)** client assets and
   operations; login and normal application functions are unavailable.
4. The person opens the **[Web UI](../../glossary.md#applications-and-interfaces)**
   over HTTPS. No account or application session exists yet, so the Web UI
   offers mutually exclusive choices to create a new deployment with Init or
   restore an existing deployment from an encrypted backup.
5. The person chooses Init and enters the new-deployment setup workflow without
   creating an account or application session.

The deployer is responsible for limiting network access to the unauthenticated
**[Pre-Operational Surfaces](../../glossary.md#applications-and-interfaces)**.
The Web UI does not ask for or imply separate proof of host control.

## Select The Application Database

1. The Web UI requests the compiled-in backend catalog from the shared
   lifecycle contract for the
   **[Application Database](../../glossary.md#applications-and-interfaces)**
   and presents each backend's typed connection fields. The MVP offers SQLite.
2. The person selects a backend, completes its fields, and submits the
   selection. For an external backend, these fields may include endpoint,
   identity, and secret connection values. The Web UI submits secret values
   only over HTTPS and never asks for a Server filesystem path or file
   reference. A local backend, including SQLite, exposes no database location
   or filename control.
3. The Server validates the request, opens the destination, and confirms that
   it is eligible for this deployment before collecting an Administrator
   password, Log Module credentials, or other application secrets.
4. On success, the lifecycle contract writes the protected database locator and
   binds it to the deployment identifier. The Server derives and manages every
   local path and file, and it encrypts any secret connection values required to
   reopen the selected database. The same running process continues setup
   without a restart.

Before recovery-key preparation, the person may return to this step and replace
the selection with another eligible database. The Server validates the
replacement before changing the locator and does not delete artifacts at the
previous destination.

## Configure Initial Logging

The Web UI collects the initial **[Log Module](../../glossary.md#applications-and-interfaces)**
configuration in this order:

1. Select and configure a Log Module for
   **[System Logs](../../glossary.md#applications-and-interfaces)**.
2. Assign that configured Log Module to receive System Logs.
3. Select and configure a Log Module for
   **[Audit Logs](../../glossary.md#applications-and-interfaces)**.
4. Assign that configured Log Module to receive Audit Logs.

The person may select the same configured Log Module for both assignments. The
Web UI makes the two assignments explicit and does not imply that a SQLite Log
Module shares the SQLite Application Database file, schema, connection, or
other resources.

## Create The First Administrator

1. The person enters the desired username and password for the first local
   **[Human User](../../glossary.md#identities-and-access)**.
2. The Web UI keeps the password only in current-page memory and submits it over
   HTTPS as part of finalization. It does not place the password in a URL,
   browser history, logs, or persistent client storage.
3. The Web UI does not ask the person to select or configure a Group. The Server
   creates the system-defined
   **[Administrators Group](../../glossary.md#identities-and-access)** and applies
   the first Human User's membership automatically during finalization.

Collecting these account details does not create or authenticate the Human User
and does not create a browser session.

## Save The Recovery Key

1. The Web UI requests recovery-key preparation only after an eligible
   **[Application Database](../../glossary.md#applications-and-interfaces)** has
   been selected.
2. The Server creates the recovery key pair, records the non-operational
   `InitializationPending` checkpoint, advances the deployment record to
   `InitializationPending`, and returns the private recovery key once over
   HTTPS.
3. The Web UI displays the private key with a clear copy action. The person is
   responsible for copying it, storing it outside Weavelit, and protecting it.
4. The Web UI requires the person to click **I saved the key** before proceeding.
   This acknowledgement records the person's responsibility; it does not verify
   that the key was copied, written to durable storage, or stored safely. The
   normal flow does not require the person to paste, upload, or reselect the key.
5. The Web UI derives the Server-required proof of possession from the private
   key held in page memory. It submits the proof, never the private key, during
   finalization and then discards its in-memory copy.

After the key is issued, database selection is immutable. The Server never
redisplays that private key.

## Review And Complete Setup

1. The Web UI presents a final review of the selected database backend, initial
   logging assignments, and first account identity. It does not redisplay
   submitted secrets.
2. The person chooses **Complete setup**.
3. The Web UI submits the normalized final request containing the account
   details, initial Log Module configurations and assignments, and recovery-key
   proof.
4. The lifecycle authority independently reloads and validates the deployment
   record, locator, database state, and matching deployment identifiers before
   Init reads request secrets or changes state.
5. The Server validates the complete request, creates the first Human User and
   the system-defined Administrators Group, adds the user to that Group,
   protects submitted secrets, and confirms that the System Log and Audit Log
   assignments can durably record their assigned log type.
6. The Application Database atomically commits the initialized application
   state.
7. The Server durably records the successful Init result through the committed
   System Log assignment, then irreversibly seals the deployment record
   `Initialized`.
8. Only after the seal is durable does the Server remove the Pre-Operational
   Surface and enable normal authenticated operation. No process restart is
   required.

If validation fails before the database commit, the Web UI keeps the person in
setup, presents an actionable redacted error, and allows correction without
generating another recovery key.

## Sign In

After successful **[Init](../../glossary.md#states-and-requests)**, the
**[Web UI](../../glossary.md#applications-and-interfaces)** redirects to its normal
sign-in screen. Init does not create an artificial or implicit application
session. The first **[Human User](../../glossary.md#identities-and-access)** enters
their new credentials and receives a Server-managed session only after normal
authentication succeeds.

## Interrupted Setup

The **[Web UI](../../glossary.md#applications-and-interfaces)** asks the lifecycle
and Init contracts for trusted **[Init](../../glossary.md#states-and-requests)**
status when setup is opened or refreshed. It never infers progress only from
browser state.

- Before database selection, setup resumes at database selection.
- After database selection but before recovery-key preparation, setup resumes
  with the selected database. User-supplied values that were held only in page
  memory must be entered again.
- After recovery-key preparation, an interruption leaves retained partial state.
   The Server exposes no Init, status, or normal route over that state and emits
   only its stable redacted operator action class.
- The Web UI does not offer reconciliation, recovery-key reset, resumed
   finalization, or any retained-state deletion or recreation action.
- The operator may preserve the failed root for diagnosis or evidence, or
   discard it and redeploy before beginning a new Init. The private recovery key
   from the failed deployment is not a way to resume that deployment.

## After Init

Once the deployment record is sealed `Initialized`, every supported
**[Init](../../glossary.md#states-and-requests)** entry point rejects further
mutation before reading secrets or changing state. Hidden or reconstructed Web
UI routes, stale requests, concurrent finalization attempts, and direct internal
calls cannot restart setup.

On later starts, the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
requires the deployment record, database locator, and Application Database to
carry the matching deployment identifier and initialized state. Missing,
malformed, unavailable, or mismatched retained components fail closed instead
of reopening Init.

## Related Documents

- [Server Init Design](../lifecycle/init/init-design.md)
- [Server Lifecycle Design](../lifecycle/lifecycle-design.md)
- [Restore User Story](restore-user-story.md)
- [Application Database Design](../database/application-database-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Testing and Validation Policy](../../testing.md)
