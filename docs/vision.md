# Weavelit Vision

Weavelit is a self-hosted gateway that lets AI agents work safely with external
services through deliberately built **[Service Modules](glossary.md#applications-and-interfaces)**. It gives an agent a
consistent way to use supported service workflows, particularly where a service
does not offer a native MCP interface that the agent can use directly.

The **[Weavelit Client](glossary.md#applications-and-interfaces)**
runs on the user's system and sends supported requests to the
**[Weavelit Server](glossary.md#applications-and-interfaces)**. The Server uses
the relevant Service Module to perform the work with the external service and
returns a structured result to the client. A **[Web UI](glossary.md#applications-and-interfaces)** is also available for administration and uses a
**[Client Module](glossary.md#applications-and-interfaces)** to assign Service
Modules and selected **[Workflows](glossary.md#applications-and-interfaces)** to
users and automations.

Weavelit provides **[Local Authentication](glossary.md#identities-and-access)**
by default, with **[External Authentication](glossary.md#identities-and-access)** available when it is a better fit. Users can create
**[Automation Identities](glossary.md#identities-and-access)** for headless AI
agents that perform scheduled or triggered work. Every automation has a human
owner who is responsible for the work it performs.

The [Core Statements](core-statements.md) define the current product and
technical commitments. The [Glossary](glossary.md) defines the canonical names
used throughout the documentation.
