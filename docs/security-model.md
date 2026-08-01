# Weavelit Security Model

This document defines Weavelit's stable security model beneath the
[Technical Specification](spec.md). It owns protected-asset classifications,
trust assumptions, cross-cutting security invariants, and approved security
profiles whose consistency must not depend on incidental implementation
choices. The Technical
Specification owns application behavior and high-level security outcomes;
component design documents own implementation structure, libraries, data
formats, control flow, and error handling.

When Weavelit standardizes a security mechanism across implementations, this
document owns the approved mechanism and minimum security profile. Component
designs explain how their implementation satisfies that profile without
weakening it. This document does not repeat requirements merely to summarize
the Technical Specification or component designs.

## Trust Boundaries And Assumptions

The Server-owned authentication, authorization, lifecycle, cryptographic, and
persistence contracts form the trusted application enforcement boundary.
Compiled-in modules operate within the Server process but remain limited to
their declared responsibilities. A
**[Client Module](glossary.md#applications-and-interfaces)** may authenticate
and translate requests, but it is not an authorization authority.

Client applications, AI agents, agent instructions, caller-supplied identity or
permission claims, request payloads, and imported backups are untrusted. The
Server validates them within the owning Server contract before they can change
trusted state or cause an external side effect. Client validation, route
visibility, and presentation controls improve usability but provide no security
authority.

This model addresses unauthorized or malformed requests, untrusted or
compromised clients and agents, credential disclosure through interfaces,
logging, or persistence, hostile imported data, authorization bypass, and
partial or failed security-sensitive workflows. It does not claim to preserve
application security after a person with sufficient host authority can replace
the Server binary, read protected Server state, or destroy all deployment
anchors.

TLS-protected HTTPS is an application invariant; the deployment operator
supplies and protects the TLS material and controls network exposure, host
access, filesystem protections, and custody of secrets retained outside
Weavelit. A
person with sufficient host authority can replace the Server binary or destroy
all persistent deployment anchors. Weavelit cannot distinguish complete
destruction of those anchors from a new installation; preventing and detecting
that action belongs to deployment access control and monitoring.

## Protected Assets

The security model protects:

- **[Human User](glossary.md#identities-and-access)** passwords, password
  verifiers, MFA provisioning values, factor data, authentication codes, and
  sessions;
- **[Automation Identity](glossary.md#identities-and-access)** credentials,
  named **[Operation](glossary.md#applications-and-interfaces)** scopes, and
  **[Responsible Owner](glossary.md#identities-and-access)** assignments;
- **[Service Connection](glossary.md#applications-and-interfaces)** credentials
  and other provider authentication material;
- Server-local at-rest keys, backup recovery private keys, unwrapped backup
  keys, and decrypted backup content;
- **[Application Database](glossary.md#applications-and-interfaces)** state and
  the integrity of the deployment record and database locator; and
- the confidentiality and integrity properties of
  **[System Logs](glossary.md#applications-and-interfaces)**,
  **[Audit Logs](glossary.md#applications-and-interfaces)**, and client-visible
  errors.

## Password And Session Security Profile

Local **[Human User](glossary.md#identities-and-access)** passwords must be
stored only with a modern adaptive
password-hashing algorithm from a maintained implementation. Passwords must
never be stored in plaintext or reversibly encrypted. The approved algorithm
and minimum protection profile belong to this security model once selected;
the Authentication Design owns the library, encoded representation, parameter
application, verification flow, and migration mechanics.

Password creation, change, reset, storage, hashing, and verification must occur
only in Server-owned authentication logic. Client applications and
**[Client Modules](glossary.md#applications-and-interfaces)** may request or
transport these workflows, but they must not persist passwords or password
verifiers or implement independent password hashing or verification. Browser
sessions must use secure, Server-managed session handling.

## Multifactor Authentication Security Profile

The initial **[Time-Based One-Time Password (TOTP)](glossary.md#identities-and-access)**
enrollment flow must confirm both the
**[Human User](glossary.md#identities-and-access)**'s current password and a
current code generated from the new provisioning value. The provisioning value
may be disclosed only to that Human User during enrollment and must never be
returned after enrollment. The Server must exclude TOTP provisioning values,
secrets, and codes before System Logs or Audit Logs reach a Log Module.

A Human User who is required to use MFA but has no valid enrollment must not
receive a usable session until enrollment in an enabled
**[MFA Module](glossary.md#applications-and-interfaces)** is complete.
Resetting an enrollment must immediately invalidate its prior factor data and,
when MFA remains required, force new enrollment before another usable session
is issued.

Before disabling an MFA Module with dependent enrollments, the Server must
report the number of affected Human Users. Disabling the module must immediately
prevent enrollment and verification through that method and terminate every
session belonging to an affected Human User. Disablement must not remove any
Human User's MFA requirement. A Human User who still requires MFA must enroll
through an enabled MFA Module; a Human User without an MFA requirement may
authenticate without MFA and may enroll through any enabled MFA Module.

An Administrator who has enrolled in MFA must complete TOTP verification for
the current session before requiring MFA for a Human User or resetting an MFA
enrollment. The Server must record MFA policy changes and resets without
recording provisioning values, secrets, or codes.

## Secret And Key Material Security Profile

Server-local at-rest key material and the backup recovery key pair must remain
distinct and must not be substituted for one another. At-rest key material
protects reversibly encrypted application values. The recovery public key
protects backup artifacts, and the corresponding private key is accepted only
to decrypt a compatible backup.

Sensitive authentication material submitted through a
**[Service Connection](glossary.md#applications-and-interfaces)** type's
declared setup workflow must terminate in Server-owned credential handling. The
Server alone stores, uses, refreshes, or revokes that material. It must not
return the material to a client, retain it in a Client Module, or include it in
an Audit Log.

Client applications must transmit initialization secrets, credentials, and
**[Restore](glossary.md#states-and-requests)** private recovery keys over HTTPS.
They must not log them, place them in URLs or browser history, or retain them in
client-side persistent storage. A Restore-capable client may read a selected
encrypted backup only to transmit it and must not retain another copy.
**[Init](glossary.md#states-and-requests)**-capable and Restore-capable Client
Modules must pass sensitive inputs only to their corresponding Server-owned
contracts and must not decrypt, log, or retain them.

The Server must never return a submitted secret and may persist it only in its
intended protected representation, such as a password verifier or encrypted
credential. The backup recovery private key must not be persisted in the
Application Database, Server configuration, a container volume, logs, or an
ordinary backup artifact. Possession of that private key authorizes decryption
of its compatible backup only; it is not an application identity, proof of host
authority, or authorization for another function.

During pre-operational Application Database selection, a client may submit
backend-declared connection values but must not submit or influence a filesystem
path or file reference. The Server must derive every local path, persist secret
connection values in encrypted form within protected Server-owned configuration,
and decrypt them only when required to open the selected database. A
client-supplied value must never cause the Server to read an unrelated local
file as connection material.

## Backup Input Security Profile

The Server must treat every submitted backup as untrusted even when the supplied
recovery key can decrypt it. Before application-state mutation, Restore must
bound upload size, cryptographic work, decompression, parsed structures,
collections, strings, execution time, and concurrency. It must validate the
backup's authenticity, integrity, format version, compatibility, references,
and complete contents. A rejected backup must leave the selected database
without application state and expose only stable, redacted errors.

Restore must not persist a private recovery key, unwrapped backup key, or
decrypted backup plaintext. If temporary staging is required, the Server may
persist only the bounded encrypted artifact in protected storage and must remove
it according to the Restore workflow's success, failure, and interruption
rules. The Restore Design owns concrete formats, algorithms, bounds, validation
order, staging mechanics, and cleanup flow.

## Log And Client Output Security Profile

The Server must remove secrets and unnecessary sensitive payload data before a
System Log or Audit Log reaches a Log Module. Log Modules receive already
redacted records and must not become an alternate sanitization boundary.

System Logs, Audit Logs, and client-visible errors must not contain passwords,
password verifiers, MFA provisioning values or codes, Service Connection
credentials, private or unwrapped keys, decrypted backup content, or other
submitted secrets. Client-visible errors must also exclude raw internal traces,
dependency-specific details, and sensitive filesystem information. Redaction
must preserve the accountability fields required for an Audit Log and the
diagnostic classification required for a System Log.

## Component Security Ownership

- The [Authentication Design](server/authentication/authentication-design.md)
  owns password, session, local MFA policy, and external-authentication
  implementation.
- MFA Module designs own method-specific enrollment, verification, and
  protected factor-data implementation; the Server retains policy,
  authorization, session, recovery, and audit authority.
- The [Authorization Design](server/authorization/authorization-design.md) owns
  grant evaluation, access-class enforcement, disablement precedence, and
  per-request authorization implementation.
- The [Automation Identity Design](server/automation-identities/automation-identity-design.md)
  owns credential lifecycle, Responsible Owner enforcement, scope mechanics,
  and accountability integration.
- Server Audit design owns Audit Log construction and pre-redaction, Server
  Observability design owns System Log construction and pre-redaction, and
  Server API design owns stable client-error presentation and redaction. Log
  Module designs own processing of records only after the Server completes the
  pre-redaction boundary.
- The [Server Lifecycle Design](server/lifecycle/lifecycle-design.md),
  [Server Init Design](server/lifecycle/init/init-design.md), and
  [Server Restore Design](server/lifecycle/restore/restore-design.md) own
  deployment-state integrity, sensitive workflow handling, and fail-closed
  lifecycle implementation.

Every component design and implementation must preserve the trust boundaries
and security profiles in this document. A component may strengthen a local
control but must not weaken, bypass, or reinterpret these invariants.

Security-sensitive behavior changes must include automated evidence under the
[Testing and Validation Policy](testing.md). Evidence must exercise the
applicable denial path, secret non-disclosure and non-persistence properties,
redaction boundary, and fail-closed behavior rather than testing only successful
requests.

## Related Documents

- [Technical Specification](spec.md)
- [Glossary](glossary.md)
- [Authentication Design](server/authentication/authentication-design.md)
- [Authorization Design](server/authorization/authorization-design.md)
- [Automation Identity Design](server/automation-identities/automation-identity-design.md)
- [Server Lifecycle Design](server/lifecycle/lifecycle-design.md)
- [Server Init Design](server/lifecycle/init/init-design.md)
- [Server Restore Design](server/lifecycle/restore/restore-design.md)
- [Testing and Validation Policy](testing.md)
