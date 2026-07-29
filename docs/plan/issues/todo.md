# Track To Do Items for Future Work

This list of to do items is a placeholder for future work that is not yet scheduled or assigned. It is not a backlog of issues, but rather a collection of ideas and tasks that may be addressed in the future. Document items here as you come across them so we can capture the thought for future consideration. When an item is ready to be scheduled, it should be moved to the appropriate issue tracker or project board. Use this list when we do not want to interrupt the current work in progress but want to capture the idea for future consideration.

## To Do Items

### Define Admin Plane and User Plane Terminology

Define and clarify **admin plane** and **user plane** as the administrative and
user-facing sections of the API and command structure exposed by a
**[Client Module](../../glossary.md#applications-and-interfaces)**. Specify the
naming, route and command organization, authorization requirements, and access
classes for each section. A Client Module may expose one or both sections: the
Web UI can provide user-plane functions and authorized admin-plane functions,
while the Weavelit CLI exposes user-plane operational functions only. Keep this
terminology distinct from the host-local
**[Admin CLI](../../glossary.md#applications-and-interfaces)** and clarify that
the planes describe application-interface organization rather than separate
network planes.

## Related Documents

- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
