# Weavelit Documentation Standards

This document defines the shared structure, writing, ownership, and maintenance
standards for production documentation under `docs/`. It gives documents a
consistent contract without forcing product references, implementation designs,
planning records, and decision records into one template.

These standards supplement the routing and boundary rules in the applicable
`AGENTS.md` files. They do not govern the structure or maintenance process of
`AGENTS.md` files themselves.

## Document Authority

Every subject has one authoritative document. The authoritative document owns
the meaning of a decision, requirement, contract, definition, or delivery
outcome and is the location where that meaning changes.

A document that depends on another document's authority must link to it. It may
briefly state the local consequence needed to make its own design or outcome
understandable, but it must not create a second independently maintained version
of the source material. When the authoritative statement changes, update its
affected consequences and links in the same change.

Use these ownership boundaries:

- `vision.md` owns the high-level product purpose and system relationships.
- `core-statements.md` owns cross-cutting settled product and technical truths.
- `security-model.md` owns cross-cutting security requirements and constraints.
- `glossary.md` owns canonical terminology and definitions.
- `open-questions.md` owns unresolved product and architecture decisions.
- Component and module design documents own implementation-specific contracts,
  invariants, lifecycle behavior, failure behavior, and technical choices.
- Planning documents own delivery outcomes and work organization, not the
  product or technical decisions on which those outcomes depend.
- Architecture decision records own the rationale and consequences of a
  consequential decision when preserving that history remains useful after the
  current design has incorporated the result.

## Shared Document Contract

Every production document must:

1. Use one level-one heading that identifies its subject.
2. Establish its purpose and authority in the opening paragraph or opening
   section.
3. State exclusions when a nearby document could reasonably appear to own the
   same material.
4. Organize content by the reader's questions and the subject's boundaries,
   not by the order in which decisions or implementation work occurred.
5. Use the canonical terms and first-substantive-use glossary links required by
   the applicable documentation guidance.
6. Keep each requirement, decision, or definition in one authoritative location
   and link to it elsewhere.
7. End with a `## Related Documents` section containing only current,
   repository-relative links to directly relevant canonical documents.

The opening does not require a heading when a short introductory paragraph
states the document's purpose and authority clearly. Add a dedicated `Scope`,
`Purpose`, or `Scope And Ownership` section when exclusions, parent-child
ownership, or applicability need more than a short paragraph.

## Document Types

Choose a document type from the information it owns. A document may combine
closely related profiles when one coherent authority genuinely requires it, but
must be split when the profiles begin to change independently.

### Canonical Product And Policy Documents

Canonical product and policy documents define current product truths,
cross-cutting requirements, terminology, or repository-wide policy. Their
structure follows the concepts they own rather than a fixed heading template.

They must distinguish current binding material from maintenance instructions
and future intent. When implementation gives a component clear ownership of
specific detail, move that detail to the component's design document and retain
only the cross-cutting rule and an authoritative link.

### Component And Module Design Documents

Design documents explain how one defined boundary satisfies product, security,
and technical requirements. They must identify:

- the component, module, contract, or lifecycle they own;
- important behavior or material explicitly outside that ownership;
- externally observable success, rejection, and failure behavior where
  applicable;
- security, persistence, compatibility, or lifecycle invariants relevant to
  the boundary; and
- validation obligations when the design introduces behavior not already
  covered by the Testing and Validation Policy.

Design documents describe the intended implementation contract, not a tour of
source files or a chronological implementation journal. Use code identifiers
when they are part of a stable contract or make the boundary materially clearer.

### Planning And Milestone Documents

Planning documents define observable delivery outcomes, sequencing, and work
organization. They may summarize a canonical requirement when needed to make a
completion outcome independently checkable, but the summary must link to its
authority and must not change or extend that requirement.

A milestone goal describes a capability, limit, protection, safe rejection, or
verification result. It does not decompose the outcome into an exhaustive list
of implementation tasks. Completion requires both the behavior and the evidence
required by the Testing and Validation Policy.

### Open Questions

An open question records a decision that is genuinely unresolved and whose
answer affects product behavior, architecture, security, operations, or a
public contract. State the decision to be made and enough settled context to
bound it without presenting an undecided option as current truth.

When resolved, remove the question and record the result in its authoritative
canonical or design document. Create an architecture decision record as well
when the rejected alternatives, rationale, or migration consequences will
remain important to future work.

### Architecture Decision Records

Use an architecture decision record for a consequential choice when future
maintainers will need to understand why it was made, which alternatives were
rejected, or what would be required to reverse it. Do not create one for routine
implementation details whose rationale is clear in the owning design.

An architecture decision record must contain:

- `Status`: proposed, accepted, superseded, or rejected;
- `Context`: the forces, constraints, and decision being addressed;
- `Decision`: the selected direction;
- `Consequences`: important benefits, costs, constraints, and follow-up work;
  and
- `Related Documents`: links to the current documents that apply the decision.

An accepted record preserves decision history; the owning canonical or design
document remains the authority for current behavior. A superseded record links
to the replacing record or current authority and is not rewritten to appear
current.

### Templates And Operational References

Templates define the information required to create another artifact. Use
explicit placeholders and instructions that are removed or replaced when the
artifact is created. Operational references define a repeatable repository or
project process and should favor ordered procedures, exact field definitions,
and verifiable outcomes over explanatory narrative.

## Document Lifecycle

Production documentation is either active, future, proposed, deprecated, or
superseded. Active is the default and requires no status label.

- A `Future` document reserves an already approved documentation boundary for
  behavior that is not implemented. It must not imply current availability.
- A `Proposed` document describes material that is under review and not yet a
  binding decision.
- A `Deprecated` document remains temporarily relevant but has a named
  replacement or removal condition.
- A `Superseded` document is retained for history and links to the authority
  that replaced it.

Place a non-active status immediately after the title or introductory paragraph
so it cannot be mistaken for current behavior. State the implication in plain
language, including where current authority remains when applicable. Do not use
dates, version fields, owners, or other metadata unless a repository process
actively maintains and consumes them.

Documentation describes current intended behavior in the present tense. Use
future tense only for explicitly future or proposed material. Do not infer that
a documented design is implemented; implementation status belongs in planning
and project systems unless the document's purpose specifically owns it.

## Structure And Splitting

Structure a document around coherent concepts whose headings help a reader
locate an answer directly. Prefer focused sections and descriptive headings over
long narrative passages. A heading should name the subject beneath it, not use
generic labels such as `Details`, `Other`, or `Notes` when a precise name is
available.

Review a document for splitting when any of these conditions is true:

- a section belongs to a different folder-level ownership boundary;
- a section changes independently from the rest of the document;
- a section has a distinct lifecycle, status, or primary audience;
- other documents repeatedly need to link directly to that section;
- keeping the section in place causes its requirements or decisions to be
  repeated elsewhere; or
- the document mixes canonical policy, component design, and delivery planning
  in ways that obscure which statements are authoritative.

Length alone does not require a split. Treat substantial growth as a prompt to
review cohesion, ownership, and navigation. Do not split a coherent document
into fragments that force readers or agents to assemble one contract from many
small files.

When extracting material, move its full authority to the new document. Replace
the old material with only the context and link needed to preserve navigation,
then update affected links and asset inventories through their established
maintenance process.

## Writing Style

- Write direct, declarative statements and use normative words consistently:
  `must` for requirements, `must not` for prohibitions, `may` for permitted
  choices, and `should` for recommendations with valid exceptions.
- Distinguish requirements from examples. Introduce examples as examples so
  they cannot be mistaken for an exhaustive contract.
- Define acronyms on first substantive use unless they are canonical glossary
  terms or universally clear in the immediate technical context.
- Use lists for genuinely parallel items, tables for comparable structured
  facts, and prose for relationships and rationale.
- Keep heading levels sequential and avoid sections that contain only a link to
  another section.
- Use repository-relative links and descriptive link text. Do not use `here`,
  raw paths, or a document title that misrepresents the linked authority.
- Prefer stable product and contract language over incidental code layout,
  temporary issue state, or implementation chronology.

## Related Documents

- [Docs Agent Guide](AGENTS.md)
- [Core Statements](core-statements.md)
- [Glossary](glossary.md)
- [Open Questions](open-questions.md)
- [Testing and Validation Policy](testing.md)
