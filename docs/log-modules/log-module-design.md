# Log Module Design

This document defines the shared design for Server-side
**[Log Modules](../glossary.md#applications-and-interfaces)**. It does not
define a destination-specific storage, delivery, retention, backup, or
migration implementation.

## Init Configuration And Assignment

During **[Init](../glossary.md#states-and-requests)**, the
**[Host Administrator](../glossary.md#identities-and-access)** selects,
configures, and activates one or more Log Modules before assigning destinations
for the two log types.

Application Database bootstrap configuration is selected and configured in a
separate preceding Init step. Selecting the same underlying technology for an
Application Database and a Log Module does not reuse its crate, code,
configuration, database file, schema, connection, or other resources. A Log
Module may instead deliver records to a non-database destination, such as email,
an API endpoint, or Checkmk; its destination type does not affect Application
Database behavior.

Interactive Init collects configuration in this order:

1. Select and configure a Log Module for **[System Logs](../glossary.md#applications-and-interfaces)**.
2. Assign that configured Log Module to receive System Logs.
3. Select and configure a Log Module for **[Audit Logs](../glossary.md#applications-and-interfaces)**.
4. Assign that configured Log Module to receive Audit Logs.

The Host Administrator may select the same configured Log Module for both
assignments. Non-interactive bootstrap configuration represents the same module
configuration and two explicit assignments, and uses the same Server-owned Init
validation as the interactive workflow.

Init rejects an absent, disabled, unconfigured, or incompatible assignment. It
also rejects an Audit Log assignment unless its configured Log Module can
durably record Audit Logs. Init remains incomplete, and the Server does not
begin normal operation, until valid System Log and Audit Log assignments are
persisted.

## Related Documents

- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Open Questions](../open-questions.md)
- [Glossary](../glossary.md)
- [Application Database Design](../server/database/application-database-design.md)
