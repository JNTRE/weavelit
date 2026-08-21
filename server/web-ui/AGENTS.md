# Server Web UI Source Agent Guide

This directory is reserved for the TypeScript and React source of the Web UI
that is bundled into the Weavelit Server package. It is the browser application
for restricted Init and Restore and authenticated self-service and
administration workflows, while the Web UI Client Module owns its Server
connection surface and final lifecycle and authorization enforcement stays with
the Server.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Web UI application source and its production asset bundle.
- It does not own browser route authentication or Server authorization policy; those belong in the Web UI Client Module and Server boundaries.
- It does not own a separately installed or released Web UI application.

## Asset Inventory

- `browser-tests/`: Playwright browser tests, covering the pre-operational status smoke test, the Application Database selection restart-persistence test, the two-request Restore submission test, the first-launch Init workflow test, sign-in, authenticated Accounts, safe lazy Group access-detail and Configuration workspaces, TOTP preview non-disclosure, and restart-persisted-session behavior, the second-factor, enrollment, and account MFA-policy workflows, and the shared fixture that runs them against the release Server binary.
- `index.html`: Vite entry document for the single-page application.
- `package.json`: Web UI manifest, exact dependency pins, and build, test, and validation scripts.
- `package-lock.json`: Fully resolved npm dependency lock for reproducible installs.
- `playwright.config.ts`: Playwright runner configuration for the browser tests.
- `scripts/`: Build-output validation and build content manifest scripts run by the Server quality gate, and their Node test-runner tests.
- `src/`: TypeScript and React application source and its unit tests, organized into `api/` (status, Application Database selection, Init setup and recovery-key proof-of-possession, Restore submission, authentication, account, Group, TOTP enablement and Log configuration administration, credential-issuance, and MFA-policy transport clients), `components/` (application shell, Init workflow, Restore submission form, sign-in form, authenticated Accounts and lazy Groups and Configuration workspaces, and Group membership/direct-grant administration), `hooks/` (deployment status hook), and `styles/` (application stylesheet), following the `weavelit-<phase>-<component>` file-naming convention.
- `tsconfig.json`: TypeScript compiler configuration for the application and its tests.
- `vite.config.ts`: Vite build, deterministic output-naming, and Vitest configuration.

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../docs/clients/web-ui/` for Web UI behavior and `../../docs/client-modules/web-ui/` for its Server connection boundary before changing a workflow.
- MUST keep presentation and client-side usability behavior here; rely on the Server
  for lifecycle, Init and Restore availability, identity derivation, and
  authorization decisions.
- MUST add focused end-to-end or smoke tests for user workflows and likely release failures, following `../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST build production Web UI assets as part of the Weavelit Server package; do not create a separate Web UI release.
- Agents MUST NOT treat client-side navigation or validation as authorization controls.
- Agents MUST NOT expose provider credentials, automation credentials, or internal error traces in the browser.
