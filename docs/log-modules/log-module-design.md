# Log Module Design

This document defines the shared design for Server-side
**[Log Modules](../glossary.md#applications-and-interfaces)**. It does not
define a destination-specific storage, delivery, retention, backup, or
migration implementation.

## Init And Restore Configuration

During **[Init](../glossary.md#states-and-requests)**, the person completing the
workflow uses an Init-capable
**[Client Module](../glossary.md#applications-and-interfaces)** administration
surface to select, configure, and activate one or more Log Modules before
assigning destinations for the two log types.

The **[Application Database](../glossary.md#applications-and-interfaces)** is
selected and configured by the shared lifecycle contract before Init accepts
Log Module configuration. Selecting the same underlying technology for an
Application Database and a Log Module does not reuse Weavelit-owned persistence
logic or implementation crates, configuration, database file, schema,
connection, or other resources. They may use the same workspace-pinned
third-party dependency, such as `rusqlite`, without sharing persistence
behavior. A Log Module may instead deliver records to a non-database
destination, such as email, an API endpoint, or Checkmk; its destination type
does not affect Application Database behavior.

The Init contract collects configuration in this order:

1. Select and configure a Log Module for **[System Logs](../glossary.md#applications-and-interfaces)**.
2. Assign that configured Log Module to receive System Logs.
3. Select and configure a Log Module for **[Audit Logs](../glossary.md#applications-and-interfaces)**.
4. Assign that configured Log Module to receive Audit Logs.

The person completing Init may select the same configured Log Module for both
assignments. Every Init-capable Client Module submits the same module
configuration and two explicit assignments to the same Server-owned
validation; no client defines an alternative Log Module initialization path.

Init rejects an absent, disabled, unconfigured, or incompatible assignment. It
also rejects an Audit Log assignment unless its configured Log Module can
durably record Audit Logs. Init remains incomplete, and the Server does not
begin normal operation, until valid System Log and Audit Log assignments are
persisted.

During **[Restore](../glossary.md#states-and-requests)**, the Server imports Log
Module configurations, enabled state, assignments, and protected credentials
from the validated Application Database backup. It does not import System Log
or Audit Log destination data. Every referenced Log Module must be compiled
into the replacement Server, and every restored configuration and assignment
must satisfy the same Server-owned validation used during normal administration.

Restore does not seal the replacement deployment until the restored Audit Log
assignment can durably record the required Restore result without recovery
secrets or backup contents. A failure remains non-operational and follows the
post-commit reconciliation rules in the
[Server Restore Design](../server/lifecycle/restore/restore-design.md). A restored Log Module
never reads backup contents or Application Database state directly.

## Related Documents

- [Technical Specification](../spec.md)
- [Security Model](../security-model.md)
- [Open Questions](../open-questions.md)
- [Glossary](../glossary.md)
- [Application Database Design](../server/database/application-database-design.md)
- [Server Restore Design](../server/lifecycle/restore/restore-design.md)
