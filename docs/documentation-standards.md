# Weavelit Documentation Standards

This document defines the shared structure, writing, and maintenance standards
for AI-maintained application documentation under `docs/`. It applies only to
documents that describe a current application area, its boundaries, and its
relationship to the rest of Weavelit; repository governance, planning, and
other non-application documents are outside its scope.

## Writing Style

- Write direct, declarative statements. The uppercase terms `MUST`, `MUST NOT`,
  `SHOULD`, `SHOULD NOT`, and `MAY` express normative requirements using their
  RFC 2119 and RFC 8174 meanings. Use ordinary prose for descriptions,
  rationale, and examples.
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

## Document Creation And Structure

Create one authoritative document for each application area. It owns the
meaning of its decisions, requirements, contracts, definitions, and delivery
outcomes, and is the location where that meaning changes.

Structure a document around coherent concepts whose headings help a reader
locate an answer directly. Prefer focused sections and descriptive headings over
long narrative passages. A heading should name the subject beneath it, not use
generic labels such as `Details`, `Other`, or `Notes` when a precise name is
available.

Split a document when its size or internal complexity makes it difficult for an
AI agent to locate, understand, and safely update specific information within a
single working context. Review a document for splitting when any of these
conditions is true:

- a section belongs to a different folder-level ownership boundary;
- a section changes independently from the rest of the document;
- a section has a distinct primary audience;
- other documents repeatedly need to link directly to that section;
- keeping the section in place causes its requirements or decisions to be
  repeated elsewhere; or
- distinct contracts or application areas are combined in a way that obscures
  their boundaries or authority.

Do not set a fixed line limit. Treat substantial growth as a signal to review
agent navigability, cohesion, and ownership. Several focused documents of about
200 lines are generally preferable to one 1,000-line document when the material
can be divided without forcing readers or agents to reconstruct one contract
from many fragments.

Split along meaningful application boundaries. For example, an API document may
separate Administration Plane and User Plane contracts, then later separate
read and write contracts only if those areas become independently navigable and
maintained.
review cohesion, ownership, and navigation. Do not split a coherent document
into fragments that force readers or agents to assemble one contract from many
small files.

When extracting material, move its full authority to the new document. Replace
the old material with only the context and link needed to preserve navigation,
then update affected links and asset inventories through their established
maintenance process.

## File Names

Application-document filenames MUST use lowercase ASCII `kebab-case` and end
in `-design.md`. The directory provides the broad application scope; the
filename MUST identify the represented application boundary or contract.

Use canonical glossary terms in filenames when they apply. When splitting a
document, each new filename MUST name its distinct boundary. Filenames MUST NOT
use sequence, temporary, date, or implementation-status labels such as
`part-2`, `new-api`, `2026-08`, or `draft`.

For example, `api-contract-design.md` may split into
`administration-plane-api-design.md` and `user-plane-api-design.md`, then into
`administration-read-api-design.md` and `administration-write-api-design.md`
when those contracts become independently maintained.

## Application Documentation Contract

Every application document must:

1. Begin with one level-one heading that identifies the represented area,
  component, contract, or subject.
2. Follow the title with a one- to five-line summary that states the document's
  purpose, authority, and covered topics.
3. Follow the summary with a `## Represented Areas` table using `Type` and
  `Link` columns. The table must identify the code, folders, crates,
  dependencies, modules, and canonical documents directly represented by the
  document, omitting categories that do not apply. Each entry must use a
  descriptive link to the relevant local source, manifest, or canonical
  document.
4. State exclusions when a nearby document could reasonably appear to own the
   same material.
5. Organize content by the reader's questions and the subject's boundaries,
   not by the order in which decisions or implementation work occurred.
6. Use the canonical terms and first-substantive-use glossary links required by
   the applicable documentation guidance.
7. Keep each requirement, decision, or definition in one authoritative location
   and link to it elsewhere.
8. End with a `## Related Documents` section containing only current,
   repository-relative links to directly relevant canonical documents.

For example:

| Type | Link |
| --- | --- |
| Crate | [weavelit-server-init](../server/crates/core/weavelit-server-init) |
| Folder | [Server init crate](../server/crates/core/weavelit-server-init) |
| Dependency | [rustls](../server/Cargo.toml) |
| Module | [secret](../server/crates/core/weavelit-server-init/src/secret.rs) |

Add a dedicated `Scope`, `Purpose`, or `Scope And Ownership` section when
exclusions, parent-child ownership, or applicability need more than the opening
summary.

## Related Documents Maintenance

Every production document MUST end with a `## Related Documents` section.
Entries MUST use non-numbered Markdown link bullets in the format
`[Description](path)` and include only valid, repository-relative links to
current, directly relevant canonical documents.

Update `Related Documents` in the same change whenever files are added, moved,
renamed, replaced, or retired. Remove stale links and add canonical links so the
section remains current.

## Related Documents

- [Docs Agent Guide](AGENTS.md)
- [Technical Specification](spec.md)
- [Glossary](glossary.md)
- [Testing and Validation Policy](testing.md)
