# Application Database Design

This document defines the shared implementation design for the Server's
internal **[Application Database](../../glossary.md#applications-and-interfaces)**
backend contract. Backend-specific storage behavior belongs in the applicable
child directory.

## Backup And Recovery

During **[Init](../../glossary.md#states-and-requests)**, the Server creates a
backup recovery key pair. The Server persists only the public key. The
**[Host Administrator](../../glossary.md#identities-and-access)** receives the
private key once and stores it outside Weavelit. This recovery key pair is not
used to protect the Server's normal database fields and is separate from the
Server-local at-rest key material used for reversibly encrypted data.

An **[Administrator](../../glossary.md#identities-and-access)** with the
**[Server Administration Permission](../../glossary.md#identities-and-access)**
can create and download an encrypted, versioned Application Database backup
through server-administration functions. For each backup, the Server creates a
fresh data-encryption key, encrypts the recovery contents with it, and protects
that data-encryption key with the persisted recovery public key. The Server
does not require, receive, store, or redisplay the private recovery key during
backup creation.

A backup includes the application configuration and state needed to restore
operational status, including local accounts, password verifiers, Groups and
their grants, enabled-module state, protected MFA factor data, Service
Connection credentials, and other application configuration. It excludes active
sessions, which are invalidated on restore. System Logs and Audit Logs are
separate Log Module data and are outside this Application Database backup
contract.

Recovery is a host-local Admin CLI operation. After a replacement Server has
completed the minimal Init required to select and configure its Application
Database, the Host Administrator supplies the backup and private recovery key.
The Server verifies backup authenticity, integrity, version compatibility, and
contents before atomically replacing the target application state. It then
protects restored reversibly encrypted data using its own Server-local at-rest
key material. The private recovery key is never persisted, logged, or included
in an ordinary backup artifact.

## Related Documents

- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Open Questions](../../open-questions.md)
- [Glossary](../../glossary.md)
