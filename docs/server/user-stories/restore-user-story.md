# Restore User Story

This document defines the user-visible first-launch
**[Restore](../../glossary.md#states-and-requests)** story for the
**[Web UI](../../glossary.md#applications-and-interfaces)**. It owns workflow
choice, user responsibilities, visible transitions, and interrupted client
behavior. The [Server Restore Design](../lifecycle/restore/restore-design.md) remains authoritative
for backup validation, recovery-key handling, persistence, concurrency, and
failure mechanics. The [Server Lifecycle Design](../lifecycle/lifecycle-design.md) owns
shared status, database selection, and route availability. Another
Restore-capable client may present a different interaction while invoking the
same Server-owned contracts.

## Install And Choose A Workflow

1. A person installs and starts the
   **[Weavelit Server](../../glossary.md#applications-and-interfaces)** package.
   Package configuration supplies the HTTPS listener, TLS material, and
   protected Server state directory.
2. On first startup, the Server creates its deployment record with a unique
   deployment identifier and `Uninitialized` lifecycle state.
3. The Server enters restricted pre-operational mode. It serves only Client
   Module assets and operations for declared Init and Restore capabilities;
   login and normal application functions are unavailable.
4. The person opens the Web UI over HTTPS. No account or application session
   exists, so the Web UI presents two mutually exclusive choices: create a new
   deployment with **[Init](../../glossary.md#states-and-requests)** or restore an
   existing deployment from an encrypted backup.
5. The person chooses Restore. The Web UI enters the Restore workflow without
   creating an account, session, or alternate host-authentication step.

The deployer is responsible for limiting network access to the unauthenticated
**[Pre-Operational Surfaces](../../glossary.md#applications-and-interfaces)**.
The Web UI does not imply that the private recovery key is proof of host
authority or a normal application credential.

## Select The Application Database

1. The Web UI requests the compiled-in
   **[Application Database](../../glossary.md#applications-and-interfaces)**
   backend catalog from the shared lifecycle contract and presents each
   backend's typed connection fields. The MVP offers SQLite.
2. The person selects a backend, completes its fields, and submits the
   selection. For an external backend, these fields may include endpoint,
   identity, and secret connection values. The Web UI submits secret values
   only over HTTPS and never asks for a Server filesystem path or file
   reference. A local backend, including SQLite, exposes no database location
   or filename control.
3. The Server validates the request, opens the destination, and confirms that
   it is empty, compatible, and eligible for the replacement deployment before
   accepting a backup or private recovery key.
4. On success, the Server writes its protected database locator and binds it to
   the replacement deployment identifier. The Server derives and manages every
   local path and file, and it encrypts any secret connection values required to
   reopen the selected database. The same running process continues Restore
   without a restart.

Before a Restore checkpoint exists, the person may return to this step and
replace the selection with another eligible database. The Server validates the
replacement before changing the locator and does not delete artifacts at the
previous destination.

## Select The Backup And Recovery Key

1. The person selects the encrypted Application Database backup from their
   local system. The Web UI retains only the browser-provided file reference
   needed for the current request and does not copy the artifact into browser
   storage.
2. The person supplies the matching private recovery key retained outside
   Weavelit when the source deployment was initialized.
3. The Web UI keeps the private key only in current-page memory and keeps the
   backup only as the browser-provided file reference. It never places the key
   in a URL, browser history, log, analytics event, crash report, or persistent
   client storage, and it clears the key as soon as the attempt it drove
   settles, whether that attempt succeeded or failed.
4. The Web UI does not inspect or preview backup plaintext, redisplay the
   recovery key, or claim that client-side checks establish validity.

The original encrypted backup remains under the person's control. Restore does
not consume or modify that source file.

## Confirm And Restore

1. The person chooses **Restore backup**.
2. The Web UI submits the private recovery key on its own first and receives a
   short-lived one-time ticket, then uploads the bounded encrypted artifact
   against that ticket. The key therefore never travels with the artifact.
   Client-side checks improve usability only; the Server independently enforces
   every bound and validation rule.
3. Before reading sensitive content or changing state, the Server reloads and
   validates the deployment record, database locator, selected database,
   deployment identifier, and Restore eligibility.
4. The Server authenticates and decrypts the backup, validates its complete
   format, compatibility, references, components, and domain state, and
   presents only stable, redacted errors if validation fails.
5. The Server invalidates restored sessions, re-encrypts protected application
   secrets with the replacement Server's at-rest key, and atomically commits
   restored application state bound to the replacement deployment identifier.
6. The Server receives the required durable acknowledgement for the Restore
   result through the restored System Log assignment and irreversibly seals the
   deployment record `Initialized`.
7. Only after the seal's configured valid-run commit path completes does the
   Server remove every pre-operational surface and enable normal authenticated
   operation. No process restart is required.

If validation fails before the database commit, the Web UI remains in Restore,
shows an actionable redacted error, discards the key from page memory when the
request ends, and does not imply that application state was partially restored.
A failed attempt consumes its ticket, so the person supplies the recovery key
again for the next attempt.

## A Backup This Server Cannot Serve

A backup can be valid, authentic, and correctly decrypted and still be refused.
A backup records the components its source deployment used, and this Server
restores one only when it compiles in every component the backup names. A backup
that enrolled an **[MFA Module](../../glossary.md#applications-and-interfaces)**
factor, configured a **[Service Module](../../glossary.md#applications-and-interfaces)**
connection, granted an operation, or assigned a
**[Log Module](../../glossary.md#applications-and-interfaces)** that this build
does not include is refused as `backup_incompatible`.

The refusal happens before any state changes, so the deployment remains eligible
for Init or another Restore. It is deliberate: restoring such a backup would
produce a deployment whose Groups, sign-in factors, and Service Connections
point at components that could never load, which would be discovered only after
the deployment was sealed. The
[Server Restore Design](../lifecycle/restore/restore-design.md#compiled-in-component-inventory)
records exactly what this build compiles in. The person's recourse is a build
that includes those components, not a modified backup; Weavelit offers no way to
drop a referenced component from a backup during Restore.

## Sign In

After successful Restore, the Web UI redirects to its normal sign-in screen.
Restore does not create an artificial or implicit browser session. A restored
**[Human User](../../glossary.md#identities-and-access)** signs in with their
restored account credentials and completes any restored MFA requirement through
normal authentication.

If no restored Administrator can authenticate because of a password or MFA
lockout, the deployment remains inaccessible through supported application
interfaces. Restore does not reopen and Weavelit provides no out-of-band
password or MFA reset. Deployment operators are responsible for maintaining
and testing backup and Administrator-account practices appropriate to their
needs; restoring a valid backup does not guarantee usable credentials or MFA
factors.

## Interrupted Restore

The Web UI asks the Server for trusted lifecycle and Restore status whenever the
workflow opens or refreshes. It never infers progress only from browser state.

- Before database selection, Restore resumes at database selection.
- After database selection but before a Restore checkpoint, Restore resumes
  with the selected database. The backup and private recovery key must be
  selected again because the Web UI did not persist them.
- Once a Restore checkpoint exists, interruption leaves retained partial state.
   The Server exposes no Init, Restore, status, or normal route over that state
   and emits only its stable redacted operator action class.
- The Web UI does not offer retry, reconciliation, safe reset, resumed upload,
   retained-state deletion, recreation, or sealing.
- The operator may preserve the failed root for diagnosis or evidence, or
   discard it and redeploy the replacement host. Restore then begins on the new
   deployment using an independently retained compatible backup and private
   recovery key. The Web UI never retains either item for this purpose.

## After Restore

Once the deployment record is sealed `Initialized`, every Restore entry point
rejects further mutation before reading a recovery key or backup content.
Hidden or reconstructed Web UI routes, stale requests, concurrent attempts, and
direct internal calls cannot restart Restore.

On later starts, the Server requires the deployment record, database locator,
and Application Database to carry the matching replacement deployment
identifier and initialized state. Missing, malformed, unavailable, or
mismatched retained components fail closed instead of reopening Init or
Restore.

## Related Documents

- [Server Restore Design](../lifecycle/restore/restore-design.md)
- [Server Lifecycle Design](../lifecycle/lifecycle-design.md)
- [Server Init Design](../lifecycle/init/init-design.md)
- [Init User Story](init-user-story.md)
- [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
- [Application Database Design](../database/application-database-design.md)
- [Security Model](../../security-model.md)
- [Testing and Validation Policy](../../testing.md)
- [Glossary](../../glossary.md)
