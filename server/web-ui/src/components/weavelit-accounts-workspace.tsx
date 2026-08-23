import { useCallback, useEffect, useRef, useState, type JSX, type SyntheticEvent } from "react";

import {
  AccountStatusRefusedError,
  AccountsSessionExpiredError,
  changeAccountStatus,
  listAccounts,
  viewAccount,
  type AccountProjection,
} from "../api/weavelit-administration-accounts";
import { probeSession } from "../api/weavelit-authentication";
import {
  CredentialIssuanceIndeterminateError,
  CredentialIssuanceSessionInvalidError,
  createAccount,
  issueCredentialIssuanceTicket,
  resetAccountPassword,
  type CredentialIssued,
} from "../api/weavelit-credential-issuance";
import {
  MfaPolicyIndeterminateError,
  MfaPolicySessionInvalidError,
  changeMfaRequirement,
  issueMfaPolicyStepUp,
  resetMfaEnrollment,
} from "../api/weavelit-mfa-policy";

type CollectionState = "loading" | "ready" | "loading-more" | "failed";
type SelectionState =
  | { readonly kind: "none" }
  | { readonly kind: "loading"; readonly publicId: string }
  | { readonly kind: "ready"; readonly account: AccountProjection }
  | { readonly kind: "failed"; readonly publicId: string };
type CredentialState =
  | "idle"
  | "assurance"
  | "issuing"
  | "consuming"
  | "checking-session"
  | "session-indeterminate"
  | "refused"
  | "indeterminate";
type PendingCredentialAction =
  | { readonly kind: "create"; readonly username: string; readonly displayName?: string }
  | { readonly kind: "reset"; readonly publicId: string; readonly username: string };
type StatusState = "idle" | "submitting" | "refused" | "indeterminate";
interface PendingStatusAction {
  readonly account: AccountProjection;
  readonly active: boolean;
}
type MfaPolicyState =
  "idle" | "confirming" | "step-up" | "issuing" | "mutating" | "refused" | "indeterminate";
type PendingMfaPolicyAction =
  | {
      readonly kind: "requirement";
      readonly account: AccountProjection;
      readonly required: boolean;
    }
  | { readonly kind: "reset"; readonly account: AccountProjection };

const FAILURE_MESSAGE = "Accounts are unavailable.";
const CREDENTIAL_REFUSED_MESSAGE = "Credential issuance was not completed.";
const CREDENTIAL_INDETERMINATE_MESSAGE =
  "The credential issuance outcome is unknown. Start a new action only if you intend to issue another credential.";
const SESSION_CHECKING_MESSAGE = "Checking whether your current session is still active.";
const SESSION_INDETERMINATE_MESSAGE =
  "The current session state could not be determined. Record this temporary password, then check again before taking another action.";
const STATUS_REFUSED_MESSAGE = "The account status was not changed.";
const STATUS_INDETERMINATE_MESSAGE =
  "The account status outcome is unknown. Refresh before taking another status action.";
const MFA_POLICY_REFUSED_MESSAGE = "MFA policy was not changed.";
const MFA_POLICY_INDETERMINATE_MESSAGE =
  "The MFA policy outcome is unknown. Refresh before taking another MFA action.";
const TOTP_PATTERN = /^[0-9]{6}$/;

export interface AccountsWorkspaceProps {
  readonly currentAccountPublicId: string;
  readonly onSessionEnded?: (disclosure?: CredentialIssued) => void;
}

export interface TemporaryPasswordDisclosureProps {
  readonly disclosure: CredentialIssued;
}

export function TemporaryPasswordDisclosure({
  disclosure,
}: TemporaryPasswordDisclosureProps): JSX.Element {
  return (
    <section className="accounts__credential-disclosure" aria-labelledby="temporary-password-title">
      <h3 id="temporary-password-title">Temporary password</h3>
      <p>This password is shown once. Record it before taking another action.</p>
      <p>
        Account public ID: <span>{disclosure.publicId}</span>
      </p>
      <label className="accounts__label" htmlFor="temporary-password-output">
        Temporary password
      </label>
      <input
        id="temporary-password-output"
        className="accounts__temporary-password"
        type="text"
        readOnly
        spellCheck={false}
        value={disclosure.temporaryPassword}
      />
    </section>
  );
}

export function AccountsWorkspace({ onSessionEnded }: AccountsWorkspaceProps): JSX.Element {
  const [accounts, setAccounts] = useState<readonly AccountProjection[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [collectionState, setCollectionState] = useState<CollectionState>("loading");
  const [selection, setSelection] = useState<SelectionState>({ kind: "none" });
  const [createUsername, setCreateUsername] = useState("");
  const [createDisplayName, setCreateDisplayName] = useState("");
  const [assurancePassword, setAssurancePassword] = useState("");
  const [assuranceTotp, setAssuranceTotp] = useState("");
  const [credentialState, setCredentialState] = useState<CredentialState>("idle");
  const [pendingCredentialAction, setPendingCredentialAction] =
    useState<PendingCredentialAction | null>(null);
  const [disclosure, setDisclosure] = useState<CredentialIssued | null>(null);
  const [statusState, setStatusState] = useState<StatusState>("idle");
  const [pendingStatusAction, setPendingStatusAction] = useState<PendingStatusAction | null>(null);
  const [mfaPolicyState, setMfaPolicyState] = useState<MfaPolicyState>("idle");
  const [pendingMfaPolicyAction, setPendingMfaPolicyAction] =
    useState<PendingMfaPolicyAction | null>(null);
  const [mfaPolicyCode, setMfaPolicyCode] = useState("");
  const mounted = useRef(true);
  const credentialAttemptActive = useRef(false);
  const statusAttemptActive = useRef(false);
  const mfaPolicyAttemptActive = useRef(false);
  const collectionRequest = useRef(0);
  const selectionRequest = useRef(0);
  const sessionEnded = useRef(false);
  const credentialIssuanceTicket = useRef("");
  const mfaPolicyTicket = useRef("");
  const workspaceIsMounted = (): boolean => mounted.current;

  const reconcileExpiredSession = useCallback(
    (error: unknown, preservedDisclosure?: CredentialIssued): boolean => {
      if (
        !(error instanceof AccountsSessionExpiredError) &&
        !(error instanceof CredentialIssuanceSessionInvalidError) &&
        !(error instanceof MfaPolicySessionInvalidError)
      ) {
        return false;
      }
      if (onSessionEnded === undefined) {
        return false;
      }
      if (!sessionEnded.current) {
        sessionEnded.current = true;
        onSessionEnded(preservedDisclosure);
      }
      return true;
    },
    [onSessionEnded],
  );

  const loadFirstPage = (preservedDisclosure?: CredentialIssued): void => {
    if (preservedDisclosure === undefined) {
      setDisclosure(null);
    }
    const request = ++collectionRequest.current;
    selectionRequest.current += 1;
    setCollectionState("loading");
    setSelection({ kind: "none" });
    void listAccounts().then(
      (page) => {
        if (mounted.current && collectionRequest.current === request) {
          setAccounts(page.items);
          setNextCursor(page.nextCursor);
          setStatusState("idle");
          setPendingStatusAction(null);
          setMfaPolicyState("idle");
          setPendingMfaPolicyAction(null);
          setMfaPolicyCode("");
          setCollectionState("ready");
        }
      },
      (error: unknown) => {
        if (!mounted.current || reconcileExpiredSession(error, preservedDisclosure)) {
          return;
        }
        if (collectionRequest.current === request) {
          setAccounts([]);
          setNextCursor(null);
          setCollectionState("failed");
        }
      },
    );
  };

  useEffect(() => {
    mounted.current = true;
    const request = ++collectionRequest.current;
    void listAccounts().then(
      (page) => {
        if (mounted.current && collectionRequest.current === request) {
          setAccounts(page.items);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      (error: unknown) => {
        if (!mounted.current || reconcileExpiredSession(error)) {
          return;
        }
        if (collectionRequest.current === request) {
          setCollectionState("failed");
        }
      },
    );
    return () => {
      mounted.current = false;
      collectionRequest.current += 1;
      selectionRequest.current += 1;
      credentialAttemptActive.current = false;
      statusAttemptActive.current = false;
      mfaPolicyAttemptActive.current = false;
      credentialIssuanceTicket.current = "";
      mfaPolicyTicket.current = "";
    };
  }, [reconcileExpiredSession]);

  const loadMore = (): void => {
    if (nextCursor === null || collectionState !== "ready") {
      return;
    }
    const request = collectionRequest.current;
    setDisclosure(null);
    setCollectionState("loading-more");
    void listAccounts(nextCursor).then(
      (page) => {
        if (mounted.current && collectionRequest.current === request) {
          setAccounts((current) => [...current, ...page.items]);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      (error: unknown) => {
        if (!mounted.current || reconcileExpiredSession(error)) {
          return;
        }
        if (collectionRequest.current === request) {
          setCollectionState("failed");
        }
      },
    );
  };

  const selectAccount = (publicId: string): void => {
    setDisclosure(null);
    const request = selectionRequest.current + 1;
    selectionRequest.current = request;
    setSelection({ kind: "loading", publicId });
    void viewAccount(publicId).then(
      (account) => {
        if (mounted.current && selectionRequest.current === request) {
          setSelection({ kind: "ready", account });
        }
      },
      (error: unknown) => {
        if (!mounted.current || reconcileExpiredSession(error)) {
          return;
        }
        if (selectionRequest.current === request) {
          setSelection({ kind: "failed", publicId });
        }
      },
    );
  };

  const beginStatusAction = (account: AccountProjection): void => {
    if (
      statusAttemptActive.current ||
      pendingStatusAction !== null ||
      credentialAttemptActive.current ||
      pendingCredentialAction !== null ||
      mfaPolicyAttemptActive.current ||
      pendingMfaPolicyAction !== null
    ) {
      return;
    }
    setDisclosure(null);
    setStatusState("idle");
    setPendingStatusAction({ account, active: !account.active });
  };

  const cancelStatusAction = (): void => {
    if (!statusAttemptActive.current) {
      setPendingStatusAction(null);
      setStatusState("idle");
    }
  };

  const submitStatusAction = (event: SyntheticEvent<HTMLFormElement, SubmitEvent>): void => {
    event.preventDefault();
    if (
      statusAttemptActive.current ||
      pendingStatusAction === null ||
      credentialAttemptActive.current ||
      mfaPolicyAttemptActive.current
    ) {
      return;
    }
    statusAttemptActive.current = true;
    setStatusState("submitting");
    const action = pendingStatusAction;

    void (async () => {
      try {
        await changeAccountStatus(action.account.publicId, action.active);
        if (!mounted.current) {
          return;
        }
        const session = await probeSession();
        if (!workspaceIsMounted()) {
          return;
        }
        setPendingStatusAction(null);
        if (session.kind === "unauthenticated") {
          setStatusState("idle");
          onSessionEnded?.();
        } else if (session.kind === "authenticated") {
          setStatusState("idle");
          loadFirstPage();
        } else {
          setStatusState("indeterminate");
        }
      } catch (error: unknown) {
        if (mounted.current) {
          setPendingStatusAction(null);
          setStatusState(error instanceof AccountStatusRefusedError ? "refused" : "indeterminate");
        }
      } finally {
        statusAttemptActive.current = false;
      }
    })();
  };

  const beginCredentialAction = (action: PendingCredentialAction): void => {
    if (
      credentialAttemptActive.current ||
      pendingCredentialAction !== null ||
      statusAttemptActive.current ||
      pendingStatusAction !== null ||
      mfaPolicyAttemptActive.current ||
      pendingMfaPolicyAction !== null
    ) {
      return;
    }
    credentialIssuanceTicket.current = "";
    setDisclosure(null);
    setAssurancePassword("");
    setAssuranceTotp("");
    setPendingCredentialAction(action);
    setCredentialState("assurance");
  };

  const beginCreate = (event: SyntheticEvent<HTMLFormElement, SubmitEvent>): void => {
    event.preventDefault();
    if (createUsername === "") {
      return;
    }
    beginCredentialAction({
      kind: "create",
      username: createUsername,
      ...(createDisplayName === "" ? {} : { displayName: createDisplayName }),
    });
  };

  const cancelCredentialAction = (): void => {
    if (credentialAttemptActive.current) {
      return;
    }
    credentialIssuanceTicket.current = "";
    setAssurancePassword("");
    setAssuranceTotp("");
    setPendingCredentialAction(null);
    setCredentialState("idle");
  };

  const reconcileSelfResetSession = async (issued: CredentialIssued): Promise<void> => {
    setCredentialState("checking-session");
    const session = await probeSession();
    if (!workspaceIsMounted()) {
      return;
    }
    if (session.kind === "unauthenticated") {
      if (onSessionEnded === undefined) {
        setCredentialState("session-indeterminate");
        return;
      }
      setCredentialState("idle");
      onSessionEnded(issued);
    } else if (session.kind === "authenticated") {
      setCredentialState("idle");
      loadFirstPage(issued);
    } else {
      setCredentialState("session-indeterminate");
    }
  };

  const recheckSelfResetSession = (): void => {
    if (
      credentialAttemptActive.current ||
      credentialState !== "session-indeterminate" ||
      disclosure === null
    ) {
      return;
    }
    credentialAttemptActive.current = true;
    void reconcileSelfResetSession(disclosure).finally(() => {
      credentialAttemptActive.current = false;
    });
  };

  const submitCredentialAction = (event: SyntheticEvent<HTMLFormElement, SubmitEvent>): void => {
    event.preventDefault();
    if (
      credentialAttemptActive.current ||
      pendingCredentialAction === null ||
      assurancePassword === "" ||
      (assuranceTotp !== "" && !TOTP_PATTERN.test(assuranceTotp))
    ) {
      return;
    }

    credentialAttemptActive.current = true;
    setCredentialState("issuing");
    const action = pendingCredentialAction;
    let password = assurancePassword;
    let totpCode: string | undefined = assuranceTotp === "" ? undefined : assuranceTotp;
    setAssurancePassword("");
    setAssuranceTotp("");

    void (async () => {
      let consuming = false;
      try {
        const ticketRequest = issueCredentialIssuanceTicket(password, totpCode);
        password = "";
        totpCode = undefined;
        let ticket = await ticketRequest;
        if (!mounted.current) {
          ticket = "";
          return;
        }
        credentialIssuanceTicket.current = ticket;
        ticket = "";
        setCredentialState("consuming");
        consuming = true;
        const consumingRequest =
          action.kind === "create"
            ? createAccount(action.username, action.displayName, credentialIssuanceTicket.current)
            : resetAccountPassword(action.publicId, credentialIssuanceTicket.current);
        credentialIssuanceTicket.current = "";
        const issued = await consumingRequest;
        if (!workspaceIsMounted()) {
          return;
        }
        setDisclosure(issued);
        setPendingCredentialAction(null);
        if (action.kind === "create") {
          setCreateUsername("");
          setCreateDisplayName("");
        }
        if (action.kind === "reset") {
          await reconcileSelfResetSession(issued);
        } else {
          setCredentialState("idle");
          loadFirstPage(issued);
        }
      } catch (error: unknown) {
        credentialIssuanceTicket.current = "";
        if (!mounted.current) {
          return;
        }
        if (reconcileExpiredSession(error)) {
          return;
        }
        if (consuming) {
          setPendingCredentialAction(null);
        }
        setCredentialState(
          error instanceof CredentialIssuanceIndeterminateError ? "indeterminate" : "refused",
        );
      } finally {
        credentialAttemptActive.current = false;
      }
    })();
  };

  const beginMfaPolicyAction = (action: PendingMfaPolicyAction): void => {
    if (
      mfaPolicyAttemptActive.current ||
      pendingMfaPolicyAction !== null ||
      credentialAttemptActive.current ||
      pendingCredentialAction !== null ||
      statusAttemptActive.current ||
      pendingStatusAction !== null
    ) {
      return;
    }
    mfaPolicyTicket.current = "";
    setDisclosure(null);
    setMfaPolicyCode("");
    setPendingMfaPolicyAction(action);
    setMfaPolicyState("confirming");
  };

  const cancelMfaPolicyAction = (): void => {
    if (mfaPolicyAttemptActive.current) {
      return;
    }
    mfaPolicyTicket.current = "";
    setMfaPolicyCode("");
    setPendingMfaPolicyAction(null);
    setMfaPolicyState("idle");
  };

  const confirmMfaPolicyAction = (): void => {
    if (pendingMfaPolicyAction !== null && mfaPolicyState === "confirming") {
      setMfaPolicyState("step-up");
    }
  };

  const submitMfaPolicyAction = (event: SyntheticEvent<HTMLFormElement, SubmitEvent>): void => {
    event.preventDefault();
    if (
      mfaPolicyAttemptActive.current ||
      pendingMfaPolicyAction === null ||
      mfaPolicyState !== "step-up" ||
      !TOTP_PATTERN.test(mfaPolicyCode)
    ) {
      return;
    }
    mfaPolicyAttemptActive.current = true;
    setMfaPolicyState("issuing");
    const action = pendingMfaPolicyAction;
    let code = mfaPolicyCode;
    setMfaPolicyCode("");

    void (async () => {
      try {
        const ticketRequest = issueMfaPolicyStepUp(code);
        code = "";
        let ticket = await ticketRequest;
        if (!mounted.current) {
          ticket = "";
          return;
        }
        mfaPolicyTicket.current = ticket;
        ticket = "";
        setMfaPolicyState("mutating");
        const mutation =
          action.kind === "requirement"
            ? changeMfaRequirement(
                action.account.publicId,
                action.required,
                mfaPolicyTicket.current,
              )
            : resetMfaEnrollment(action.account.publicId, mfaPolicyTicket.current);
        mfaPolicyTicket.current = "";
        await mutation;
        if (!workspaceIsMounted()) {
          return;
        }
        const session = await probeSession();
        if (!workspaceIsMounted()) {
          return;
        }
        setPendingMfaPolicyAction(null);
        if (session.kind === "unauthenticated") {
          setMfaPolicyState("idle");
          onSessionEnded?.();
        } else if (session.kind === "authenticated") {
          setMfaPolicyState("idle");
          loadFirstPage();
        } else {
          setMfaPolicyState("indeterminate");
        }
      } catch (error: unknown) {
        mfaPolicyTicket.current = "";
        if (mounted.current) {
          setPendingMfaPolicyAction(null);
          if (reconcileExpiredSession(error)) {
            setMfaPolicyState("idle");
          } else {
            setMfaPolicyState(
              error instanceof MfaPolicyIndeterminateError ? "indeterminate" : "refused",
            );
          }
        }
      } finally {
        mfaPolicyTicket.current = "";
        mfaPolicyAttemptActive.current = false;
      }
    })();
  };

  const credentialBusy =
    credentialState === "issuing" ||
    credentialState === "consuming" ||
    credentialState === "checking-session";
  const credentialActionLocked =
    pendingCredentialAction !== null ||
    credentialBusy ||
    credentialState === "session-indeterminate";
  const statusBusy = statusState === "submitting";
  const mfaPolicyBusy = mfaPolicyState === "issuing" || mfaPolicyState === "mutating";
  const mfaPolicyActionLocked = pendingMfaPolicyAction !== null || mfaPolicyBusy;
  const mutationReconciliationRequired =
    statusState === "indeterminate" || mfaPolicyState === "indeterminate";
  const actionLocked =
    credentialActionLocked ||
    pendingStatusAction !== null ||
    statusBusy ||
    mfaPolicyActionLocked ||
    mutationReconciliationRequired;
  const refreshActionLocked = actionLocked && !mutationReconciliationRequired;
  const validAssuranceTotp = assuranceTotp === "" || TOTP_PATTERN.test(assuranceTotp);

  return (
    <section
      className="accounts"
      data-accounts-state={collectionState}
      data-credential-state={credentialState}
      data-account-status-state={statusState}
      data-mfa-policy-state={mfaPolicyState}
    >
      <header className="accounts__toolbar">
        <div>
          <h2 className="accounts__title">Accounts</h2>
          <p className="accounts__count">{accounts.length} loaded</p>
        </div>
        <button
          type="button"
          className="accounts__action"
          onClick={() => {
            loadFirstPage();
          }}
          disabled={
            collectionState === "loading" ||
            collectionState === "loading-more" ||
            refreshActionLocked
          }
        >
          Refresh
        </button>
      </header>

      {collectionState === "loading" ? <p role="status">Loading accounts.</p> : null}
      {collectionState === "failed" ? (
        <div className="accounts__failure" role="alert">
          <p>{FAILURE_MESSAGE}</p>
          <button
            type="button"
            className="accounts__action"
            onClick={() => {
              if (!refreshActionLocked) {
                loadFirstPage();
              }
            }}
            disabled={refreshActionLocked}
          >
            Retry
          </button>
        </div>
      ) : null}

      <section className="accounts__create" aria-labelledby="account-create-title">
        <h3 id="account-create-title">Create account</h3>
        <form onSubmit={beginCreate}>
          <label className="accounts__label" htmlFor="account-create-username">
            Username
          </label>
          <input
            id="account-create-username"
            className="accounts__input"
            type="text"
            autoComplete="off"
            spellCheck={false}
            value={createUsername}
            onChange={(event) => {
              setCreateUsername(event.target.value);
            }}
            disabled={actionLocked}
          />
          <label className="accounts__label" htmlFor="account-create-display-name">
            Display name (optional)
          </label>
          <input
            id="account-create-display-name"
            className="accounts__input"
            type="text"
            autoComplete="off"
            value={createDisplayName}
            onChange={(event) => {
              setCreateDisplayName(event.target.value);
            }}
            disabled={credentialActionLocked}
          />
          <button
            type="submit"
            className="accounts__action"
            disabled={actionLocked || createUsername === ""}
          >
            Create account
          </button>
        </form>
      </section>

      {pendingCredentialAction !== null ? (
        <section className="accounts__assurance" aria-labelledby="credential-assurance-title">
          <h3 id="credential-assurance-title">Credential assurance</h3>
          <p>
            Confirm with your current password
            {pendingCredentialAction.kind === "create"
              ? ` to create ${pendingCredentialAction.username}.`
              : ` to reset ${pendingCredentialAction.username}.`}
          </p>
          <form onSubmit={submitCredentialAction}>
            <label className="accounts__label" htmlFor="credential-assurance-password">
              Current password
            </label>
            <input
              id="credential-assurance-password"
              className="accounts__input"
              type="password"
              autoComplete="current-password"
              spellCheck={false}
              value={assurancePassword}
              onChange={(event) => {
                setAssurancePassword(event.target.value);
              }}
              disabled={credentialBusy}
            />
            <label className="accounts__label" htmlFor="credential-assurance-totp">
              Authentication code (optional)
            </label>
            <input
              id="credential-assurance-totp"
              className="accounts__input"
              type="text"
              inputMode="numeric"
              autoComplete="one-time-code"
              spellCheck={false}
              maxLength={6}
              value={assuranceTotp}
              onChange={(event) => {
                setAssuranceTotp(event.target.value);
              }}
              disabled={credentialBusy}
            />
            <button
              type="submit"
              className="accounts__action"
              disabled={credentialBusy || assurancePassword === "" || !validAssuranceTotp}
            >
              Confirm credential issuance
            </button>
            <button
              type="button"
              className="accounts__action"
              onClick={cancelCredentialAction}
              disabled={credentialBusy}
            >
              Cancel
            </button>
          </form>
        </section>
      ) : null}

      {credentialState === "refused" ? (
        <p className="accounts__credential-status" role="alert">
          {CREDENTIAL_REFUSED_MESSAGE}
        </p>
      ) : null}
      {credentialState === "indeterminate" ? (
        <p className="accounts__credential-status" role="status">
          {CREDENTIAL_INDETERMINATE_MESSAGE}
        </p>
      ) : null}

      {disclosure !== null ? <TemporaryPasswordDisclosure disclosure={disclosure} /> : null}

      {credentialState === "checking-session" ? (
        <p className="accounts__credential-status" role="status">
          {SESSION_CHECKING_MESSAGE}
        </p>
      ) : null}
      {credentialState === "session-indeterminate" ? (
        <div className="accounts__credential-status">
          <p role="status">{SESSION_INDETERMINATE_MESSAGE}</p>
          <button type="button" className="accounts__action" onClick={recheckSelfResetSession}>
            Check session again
          </button>
        </div>
      ) : null}

      {pendingStatusAction !== null ? (
        <section
          className="accounts__status-confirmation"
          aria-labelledby="account-status-confirmation-title"
        >
          <h3 id="account-status-confirmation-title">
            {pendingStatusAction.active ? "Re-enable account" : "Disable account"}
          </h3>
          <p>
            {pendingStatusAction.active
              ? `Re-enable ${pendingStatusAction.account.username}?`
              : `Disable ${pendingStatusAction.account.username}? This ends every session for the account, including your current session if this is your account.`}
          </p>
          <form onSubmit={submitStatusAction}>
            <button type="submit" className="accounts__action" disabled={statusBusy}>
              {pendingStatusAction.active ? "Confirm re-enable" : "Confirm disable"}
            </button>
            <button
              type="button"
              className="accounts__action"
              onClick={cancelStatusAction}
              disabled={statusBusy}
            >
              Cancel
            </button>
          </form>
        </section>
      ) : null}

      {statusState === "refused" ? (
        <p className="accounts__status-result" role="alert">
          {STATUS_REFUSED_MESSAGE}
        </p>
      ) : null}
      {statusState === "indeterminate" ? (
        <p className="accounts__status-result" role="status">
          {STATUS_INDETERMINATE_MESSAGE}
        </p>
      ) : null}

      {pendingMfaPolicyAction !== null ? (
        <section
          className="accounts__mfa-confirmation"
          aria-labelledby="mfa-policy-title"
          role="dialog"
          aria-modal="true"
        >
          <h3 id="mfa-policy-title">
            {pendingMfaPolicyAction.kind === "reset"
              ? "Reset MFA enrollment"
              : pendingMfaPolicyAction.required
                ? "Require MFA"
                : "Make MFA optional"}
          </h3>
          {mfaPolicyState === "confirming" ? (
            <>
              <p>
                {pendingMfaPolicyAction.kind === "reset"
                  ? `Reset MFA for ${pendingMfaPolicyAction.account.username}? This removes the enrolled factor and ends every session for the account.`
                  : pendingMfaPolicyAction.required
                    ? `Require MFA for ${pendingMfaPolicyAction.account.username}? This ends every session for the account.`
                    : `Make MFA optional for ${pendingMfaPolicyAction.account.username}?`}
              </p>
              <button type="button" className="accounts__action" onClick={confirmMfaPolicyAction}>
                Confirm policy action
              </button>
              <button type="button" className="accounts__action" onClick={cancelMfaPolicyAction}>
                Cancel
              </button>
            </>
          ) : (
            <form onSubmit={submitMfaPolicyAction}>
              <label className="accounts__label" htmlFor="mfa-policy-code">
                Authentication code
              </label>
              <input
                id="mfa-policy-code"
                className="accounts__input"
                type="text"
                inputMode="numeric"
                autoComplete="one-time-code"
                spellCheck={false}
                maxLength={6}
                value={mfaPolicyCode}
                onChange={(event) => {
                  setMfaPolicyCode(event.target.value);
                }}
                disabled={mfaPolicyBusy}
              />
              <button
                type="submit"
                className="accounts__action"
                disabled={mfaPolicyBusy || !TOTP_PATTERN.test(mfaPolicyCode)}
              >
                Verify and apply
              </button>
              <button
                type="button"
                className="accounts__action"
                onClick={cancelMfaPolicyAction}
                disabled={mfaPolicyBusy}
              >
                Cancel
              </button>
            </form>
          )}
        </section>
      ) : null}

      {mfaPolicyState === "refused" ? (
        <p className="accounts__mfa-result" role="alert">
          {MFA_POLICY_REFUSED_MESSAGE}
        </p>
      ) : null}
      {mfaPolicyState === "indeterminate" ? (
        <p className="accounts__mfa-result" role="status">
          {MFA_POLICY_INDETERMINATE_MESSAGE}
        </p>
      ) : null}

      {collectionState === "ready" || collectionState === "loading-more" ? (
        <>
          <div className="accounts__table-wrap">
            <table className="accounts__table">
              <thead>
                <tr>
                  <th scope="col">Username</th>
                  <th scope="col">Display name</th>
                  <th scope="col">Status</th>
                  <th scope="col">MFA</th>
                  <th scope="col">
                    <span className="accounts__visually-hidden">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {accounts.map((account) => (
                  <tr key={account.publicId}>
                    <th scope="row">{account.username}</th>
                    <td>{account.displayName ?? "-"}</td>
                    <td>{account.active ? "Active" : "Disabled"}</td>
                    <td>{account.mfaRequired ? "Required" : "Optional"}</td>
                    <td className="accounts__row-action">
                      <button
                        type="button"
                        className="accounts__view"
                        onClick={() => {
                          selectAccount(account.publicId);
                        }}
                        disabled={actionLocked}
                      >
                        View
                      </button>
                      <button
                        type="button"
                        className="accounts__view"
                        aria-label={`Reset password for ${account.username}`}
                        onClick={() => {
                          beginCredentialAction({
                            kind: "reset",
                            publicId: account.publicId,
                            username: account.username,
                          });
                        }}
                        disabled={actionLocked}
                      >
                        Reset password
                      </button>
                      <button
                        type="button"
                        className="accounts__view"
                        aria-label={`${account.active ? "Disable" : "Re-enable"} ${account.username}`}
                        onClick={() => {
                          beginStatusAction(account);
                        }}
                        disabled={actionLocked}
                      >
                        {account.active ? "Disable" : "Re-enable"}
                      </button>
                      <label className="accounts__mfa-switch">
                        <input
                          type="checkbox"
                          role="switch"
                          aria-label={`Require MFA for ${account.username}`}
                          checked={account.mfaRequired}
                          onChange={() => {
                            beginMfaPolicyAction({
                              kind: "requirement",
                              account,
                              required: !account.mfaRequired,
                            });
                          }}
                          disabled={actionLocked}
                        />
                        MFA required
                      </label>
                      <button
                        type="button"
                        className="accounts__view"
                        aria-label={`Reset MFA for ${account.username}`}
                        onClick={() => {
                          beginMfaPolicyAction({ kind: "reset", account });
                        }}
                        disabled={actionLocked}
                      >
                        Reset MFA
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {nextCursor !== null ? (
            <button
              type="button"
              className="accounts__action"
              onClick={loadMore}
              disabled={collectionState === "loading-more" || actionLocked}
            >
              Load more
            </button>
          ) : null}
        </>
      ) : null}

      {selection.kind === "loading" ? <p role="status">Loading account.</p> : null}
      {selection.kind === "failed" ? <p role="alert">{FAILURE_MESSAGE}</p> : null}
      {selection.kind === "ready" ? (
        <section className="accounts__detail" aria-labelledby="account-detail-title">
          <h3 id="account-detail-title">{selection.account.username}</h3>
          <dl>
            <div>
              <dt>Public ID</dt>
              <dd>{selection.account.publicId}</dd>
            </div>
            <div>
              <dt>Display name</dt>
              <dd>{selection.account.displayName ?? "-"}</dd>
            </div>
            <div>
              <dt>Status</dt>
              <dd>{selection.account.active ? "Active" : "Disabled"}</dd>
            </div>
            <div>
              <dt>MFA</dt>
              <dd>{selection.account.mfaRequired ? "Required" : "Optional"}</dd>
            </div>
          </dl>
          <button
            type="button"
            className="accounts__action accounts__detail-action"
            aria-label={`${selection.account.active ? "Disable" : "Re-enable"} ${selection.account.username} from account detail`}
            onClick={() => {
              beginStatusAction(selection.account);
            }}
            disabled={actionLocked}
          >
            {selection.account.active ? "Disable" : "Re-enable"}
          </button>
          <label className="accounts__mfa-switch accounts__detail-action">
            <input
              type="checkbox"
              role="switch"
              aria-label={`Require MFA for ${selection.account.username} from account detail`}
              checked={selection.account.mfaRequired}
              onChange={() => {
                beginMfaPolicyAction({
                  kind: "requirement",
                  account: selection.account,
                  required: !selection.account.mfaRequired,
                });
              }}
              disabled={actionLocked}
            />
            MFA required
          </label>
          <button
            type="button"
            className="accounts__action accounts__detail-action"
            aria-label={`Reset MFA for ${selection.account.username} from account detail`}
            onClick={() => {
              beginMfaPolicyAction({ kind: "reset", account: selection.account });
            }}
            disabled={actionLocked}
          >
            Reset MFA
          </button>
        </section>
      ) : null}
    </section>
  );
}
