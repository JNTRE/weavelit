import { useEffect, useState, type JSX } from "react";

import {
  listAccounts,
  viewAccount,
  type AccountProjection,
} from "../api/weavelit-administration-accounts";

type CollectionState = "loading" | "ready" | "loading-more" | "failed";
type SelectionState =
  | { readonly kind: "none" }
  | { readonly kind: "loading"; readonly publicId: string }
  | { readonly kind: "ready"; readonly account: AccountProjection }
  | { readonly kind: "failed"; readonly publicId: string };

const FAILURE_MESSAGE = "Accounts are unavailable.";

export function AccountsWorkspace(): JSX.Element {
  const [accounts, setAccounts] = useState<readonly AccountProjection[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [collectionState, setCollectionState] = useState<CollectionState>("loading");
  const [selection, setSelection] = useState<SelectionState>({ kind: "none" });

  const loadFirstPage = (): void => {
    setCollectionState("loading");
    setSelection({ kind: "none" });
    void listAccounts().then(
      (page) => {
        setAccounts(page.items);
        setNextCursor(page.nextCursor);
        setCollectionState("ready");
      },
      () => {
        setAccounts([]);
        setNextCursor(null);
        setCollectionState("failed");
      },
    );
  };

  useEffect(() => {
    let active = true;
    void listAccounts().then(
      (page) => {
        if (active) {
          setAccounts(page.items);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      () => {
        if (active) {
          setCollectionState("failed");
        }
      },
    );
    return () => {
      active = false;
    };
  }, []);

  const loadMore = (): void => {
    if (nextCursor === null || collectionState !== "ready") {
      return;
    }
    setCollectionState("loading-more");
    void listAccounts(nextCursor).then(
      (page) => {
        setAccounts((current) => [...current, ...page.items]);
        setNextCursor(page.nextCursor);
        setCollectionState("ready");
      },
      () => {
        setCollectionState("failed");
      },
    );
  };

  const selectAccount = (publicId: string): void => {
    setSelection({ kind: "loading", publicId });
    void viewAccount(publicId).then(
      (account) => {
        setSelection({ kind: "ready", account });
      },
      () => {
        setSelection({ kind: "failed", publicId });
      },
    );
  };

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
          onClick={loadFirstPage}
          disabled={collectionState === "loading" || collectionState === "loading-more"}
        >
          Refresh
        </button>
      </header>

      {collectionState === "loading" ? <p role="status">Loading accounts.</p> : null}
      {collectionState === "failed" ? (
        <div className="accounts__failure" role="alert">
          <p>{FAILURE_MESSAGE}</p>
          <button type="button" className="accounts__action" onClick={loadFirstPage}>
            Retry
          </button>
        </div>
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
                      >
                        View
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
