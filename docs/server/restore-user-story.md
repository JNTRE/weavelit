# Restore User Story

This document defines the user-visible first-launch
**[Restore](../glossary.md#states-and-requests)** story for the
**[Web UI](../glossary.md#applications-and-interfaces)**. It owns workflow
choice, user responsibilities, visible transitions, and interrupted client
behavior. The [Server Restore Design](restore-design.md) remains authoritative
for backup validation, recovery-key handling, persistence, concurrency, and
failure mechanics. The [Server Lifecycle Design](lifecycle-design.md) owns
shared status, database selection, and route availability. Another
Restore-capable client may present a different interaction while invoking the
same Server-owned contracts.

## Install And Choose A Workflow

1. A person installs and starts the
   **[Weavelit Server](../glossary.md#applications-and-interfaces)** package.
   Package configuration supplies the HTTPS listener, TLS material, and
   protected Server state directory.
2. On first startup, the Server creates its deployment record with a unique
   deployment identifier and `Uninitialized` lifecycle state.
3. The Server enters restricted pre-operational mode. It serves only Client
   Module assets and operations for declared Init and Restore capabilities;
   login and normal application functions are unavailable.
4. The person opens the Web UI over HTTPS. No account or application session
   exists, so the Web UI presents two mutually exclusive choices: create a new
   deployment with **[Init](../glossary.md#states-and-requests)** or restore an
   existing deployment from an encrypted backup.
5. The person chooses Restore. The Web UI enters the Restore workflow without
   creating an account, session, or alternate host-authentication step.

The deployer is responsible for limiting network access to the unauthenticated
pre-operational surfaces. The Web UI does not imply that the private recovery
key is proof of host authority or a normal application credential.

## Select The Application Database

1. The Web UI requests the compiled-in
   **[Application Database](../glossary.md#applications-and-interfaces)**
   backend catalog from the shared lifecycle contract and presents each
   backend's typed configuration fields. The MVP offers SQLite.
2. The person selects a backend, completes its fields, and submits the
   selection. Secret connection material is represented only by supported
   secret-file references.
3. The Server validates the request, opens the destination, and confirms that
   it is empty, compatible, and eligible for the replacement deployment before
   accepting a backup or private recovery key.
4. On success, the Server writes its protected database locator and binds it to
   the replacement deployment identifier. The same running process continues
   Restore without a restart.

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
3. The Web UI keeps the private key only in current-page memory. It never places
   the key in a URL, browser history, log, analytics event, crash report, or
   persistent client storage.
4. The Web UI presents a review of the selected destination backend and the
   locally selected artifact. It does not inspect or preview backup plaintext,
   redisplay the recovery key, or claim that client-side checks establish
   validity.

The original encrypted backup remains under the person's control. Restore does
not consume or modify that source file.

## Confirm And Restore

1. The person chooses **Restore deployment**.
2. The Web UI sends the bounded encrypted artifact and private recovery key over
   HTTPS to the Restore-capable Web UI Client Module. Client-side size and format
   checks improve usability only; the Server independently enforces every
   bound and validation rule.
3. Before reading sensitive content or changing state, the Server reloads and
   validates the deployment record, database locator, selected database,
   deployment identifier, and Restore eligibility.
4. The Server authenticates and decrypts the backup, validates its complete
   format, compatibility, references, components, and domain state, and
   presents only stable, redacted errors if validation fails.
5. The Server invalidates restored sessions, re-encrypts protected application
   secrets with the replacement Server's at-rest key, and atomically commits
   restored application state bound to the replacement deployment identifier.
6. The Server durably records the required Restore result through the restored
   Audit Log assignment and irreversibly seals the deployment record
   `Initialized`.
7. Only after the seal is durable does the Server remove every pre-operational
   surface and enable normal authenticated operation. No process restart is
   required.

If validation fails before the database commit, the Web UI remains in Restore,
shows an actionable redacted error, discards the key from page memory when the
request ends, and does not imply that application state was partially restored.

## Sign In

After successful Restore, the Web UI redirects to its normal sign-in screen.
Restore does not create an artificial or implicit browser session. A restored
**[Human User](../glossary.md#identities-and-access)** signs in with their
restored account credentials and completes any restored MFA requirement through
normal authentication.

If no restored Administrator can authenticate because of a password or MFA
lockout, the deployment remains inaccessible through supported application
interfaces. Restore does not reopen and Weavelit provides no out-of-band
password or MFA reset. Deployment operators are responsible for maintaining
and testing backup and Administrator-account practices appropriate to their
needs; restoring a valid backup does not guarantee usable credentials or MFA
factors.

## Resume Interrupted Restore

The Web UI asks the Server for trusted lifecycle and Restore status whenever the
workflow opens or refreshes. It never infers progress only from browser state.

- Before database selection, Restore resumes at database selection.
- After database selection but before a Restore checkpoint, Restore resumes
  with the selected database. The backup and private recovery key must be
  selected again because the Web UI did not persist them.
- Once a Restore checkpoint exists, Init and database replacement remain
  unavailable. The Web UI follows the Server-reported Restore retry,
  reconciliation, or safe-reset options.
- Depending on the selected artifact-staging policy, retry may require the
  person to reselect the same encrypted backup and private key or may resume a
  protected encrypted upload. The Web UI never persists the key for that retry.
- A safe reset is offered only when the Server proves that no application state
  committed. Reset removes pending Restore state and never redisplays or
  reconstructs the private recovery key.
- If application state commits but Restore-result Audit Log recording or
  deployment sealing is interrupted, the Server exposes no routes. On restart
  it completes Restore-specific reconciliation and sealing before presenting
  normal sign-in; it never reopens Init or the upload workflow.

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

- [Server Restore Design](restore-design.md)
- [Server Lifecycle Design](lifecycle-design.md)
- [Server Init Design](init-design.md)
- [Init User Story](init-user-story.md)
- [Application Database Design](database/application-database-design.md)
- [Security Model](../security-model.md)
- [Testing and Validation Policy](../testing.md)
- [Glossary](../glossary.md)
