# Weavelit Vision

Weavelit is a self-hosted gateway that lets AI agents work safely with external
services through deliberately built **[Service Modules](glossary.md#applications-and-interfaces)**. It gives an agent a
consistent way to invoke supported **[Operations](glossary.md#applications-and-interfaces)**, particularly where a service
does not offer a native MCP interface that the agent can use directly.

The **[Weavelit CLI](glossary.md#applications-and-interfaces)**
runs on the user's system and sends supported requests to the
**[Weavelit Server](glossary.md#applications-and-interfaces)**. The Server uses
the relevant Service Module to perform the work with the external service and
returns a structured result to the client. A **[Web UI](glossary.md#applications-and-interfaces)** uses a
**[Client Module](glossary.md#applications-and-interfaces)** to connect to the
Server, provide permitted **[Human Users](glossary.md#identities-and-access)**
with self-service account functions, and provide
**[Administrators](glossary.md#identities-and-access)** with management
functions.

Weavelit provides **[Local Authentication](glossary.md#identities-and-access)**
by default, with **[External Authentication](glossary.md#identities-and-access)** available when it is a better fit. Users can create
**[Automation Identities](glossary.md#identities-and-access)** for headless AI
agents that perform scheduled or triggered work. Every automation has an active
**[Responsible Owner](glossary.md#identities-and-access)** who is accountable
for the work it performs.

The [Core Statements](core-statements.md) define the current product and
technical commitments. The [Glossary](glossary.md) defines the canonical names
used throughout the documentation.

## Related Documents

- [Core Statements](core-statements.md)
- [Security Model](security-model.md)
- [Glossary](glossary.md)
