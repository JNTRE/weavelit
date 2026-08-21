import { useEffect, useRef, useState, type JSX, type SyntheticEvent } from "react";

import {
  listAccounts,
  viewAccount,
  type AccountProjection,
} from "../api/weavelit-administration-accounts";
import {
  CredentialIssuanceIndeterminateError,
  createAccount,
  issueCredentialIssuanceTicket,
  resetAccountPassword,
  type CredentialIssued,
} from "../api/weavelit-credential-issuance";

type CollectionState = "loading" | "ready" | "loading-more" | "failed";
type SelectionState =
  | { readonly kind: "none" }
  | { readonly kind: "loading"; readonly publicId: string }
  | { readonly kind: "ready"; readonly account: AccountProjection }
  | { readonly kind: "failed"; readonly publicId: string };
type CredentialState = "idle" | "assurance" | "issuing" | "consuming" | "refused" | "indeterminate";
type PendingCredentialAction =
  | { readonly kind: "create"; readonly username: string; readonly displayName?: string }
  | { readonly kind: "reset"; readonly publicId: string; readonly username: string };

const FAILURE_MESSAGE = "Accounts are unavailable.";
const CREDENTIAL_REFUSED_MESSAGE = "Credential issuance was not completed.";
const CREDENTIAL_INDETERMINATE_MESSAGE =
  "The credential issuance outcome is unknown. Start a new action only if you intend to issue another credential.";
const TOTP_PATTERN = /^[0-9]{6}$/;

export function AccountsWorkspace(): JSX.Element {
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
  const mounted = useRef(true);
  const credentialAttemptActive = useRef(false);
  const credentialIssuanceTicket = useRef("");
  const workspaceIsMounted = (): boolean => mounted.current;

  const loadFirstPage = (preserveDisclosure = false): void => {
    if (!preserveDisclosure) {
      setDisclosure(null);
    }
    setCollectionState("loading");
    setSelection({ kind: "none" });
    void listAccounts().then(
      (page) => {
        if (mounted.current) {
          setAccounts(page.items);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      () => {
        if (mounted.current) {
          setAccounts([]);
          setNextCursor(null);
          setCollectionState("failed");
        }
      },
    );
  };

  useEffect(() => {
    mounted.current = true;
    void listAccounts().then(
      (page) => {
        if (mounted.current) {
          setAccounts(page.items);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      () => {
        if (mounted.current) {
          setCollectionState("failed");
        }
      },
    );
    return () => {
      mounted.current = false;
      credentialAttemptActive.current = false;
      credentialIssuanceTicket.current = "";
    };
  }, []);

  const loadMore = (): void => {
    if (nextCursor === null || collectionState !== "ready") {
      return;
    }
    setDisclosure(null);
    setCollectionState("loading-more");
    void listAccounts(nextCursor).then(
      (page) => {
        if (mounted.current) {
          setAccounts((current) => [...current, ...page.items]);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      () => {
        if (mounted.current) {
          setCollectionState("failed");
        }
      },
    );
  };

  const selectAccount = (publicId: string): void => {
    setDisclosure(null);
    setSelection({ kind: "loading", publicId });
    void viewAccount(publicId).then(
      (account) => {
        if (mounted.current) {
          setSelection({ kind: "ready", account });
        }
      },
      () => {
        if (mounted.current) {
          setSelection({ kind: "failed", publicId });
        }
      },
    );
  };

  const beginCredentialAction = (action: PendingCredentialAction): void => {
    if (credentialAttemptActive.current || pendingCredentialAction !== null) {
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
        setCredentialState("idle");
        if (action.kind === "create") {
          setCreateUsername("");
          setCreateDisplayName("");
        }
        loadFirstPage(true);
      } catch (error: unknown) {
        credentialIssuanceTicket.current = "";
        if (!mounted.current) {
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

  const credentialBusy = credentialState === "issuing" || credentialState === "consuming";
  const credentialActionLocked = pendingCredentialAction !== null || credentialBusy;
  const validAssuranceTotp = assuranceTotp === "" || TOTP_PATTERN.test(assuranceTotp);

  return (
    <section className="accounts" data-accounts-state={collectionState}>
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
          disabled={collectionState === "loading" || collectionState === "loading-more"}
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
              loadFirstPage();
            }}
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
            disabled={credentialActionLocked}
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
            disabled={credentialActionLocked || createUsername === ""}
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

      {disclosure !== null ? (
        <section
          className="accounts__credential-disclosure"
          aria-labelledby="temporary-password-title"
        >
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
                        disabled={credentialActionLocked}
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
                        disabled={credentialActionLocked}
                      >
                        Reset password
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
              disabled={collectionState === "loading-more"}
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
        </section>
      ) : null}
    </section>
  );
}
