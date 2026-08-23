import { useCallback, useEffect, useRef, useState, type JSX, type SyntheticEvent } from "react";

import {
  GroupAdministrationAccessDeniedError,
  GroupMutationRefusedError,
  GroupSessionInvalidError,
  createGroup,
  deleteGroup,
  listGroups,
  updateGroup,
  viewGroup,
  type GroupProjection,
} from "../api/weavelit-administration-groups";
import {
  MfaPolicyGrantMutationAccessDeniedError,
  MfaPolicyRefusedError,
  MfaPolicySessionInvalidError,
  issueGrantMutationStepUp,
} from "../api/weavelit-mfa-policy";
import { GroupAssociations } from "./weavelit-group-associations";

type CollectionState = "loading" | "ready" | "loading-more" | "failed";
type MutationState = "idle" | "submitting" | "refused" | "indeterminate";
type DeleteState = "idle" | "confirming" | "step-up" | "deleting" | "refused" | "indeterminate";

const TOTP_PATTERN = /^[0-9]{6}$/;

export interface GroupsWorkspaceProps {
  readonly onAdministrationEnded?: () => void;
}

export function GroupsWorkspace({ onAdministrationEnded }: GroupsWorkspaceProps = {}): JSX.Element {
  const [groups, setGroups] = useState<readonly GroupProjection[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [collectionState, setCollectionState] = useState<CollectionState>("loading");
  const [selected, setSelected] = useState<GroupProjection | null>(null);
  const [createName, setCreateName] = useState("");
  const [createDescription, setCreateDescription] = useState("");
  const [editName, setEditName] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [mutationState, setMutationState] = useState<MutationState>("idle");
  const [refreshRequired, setRefreshRequired] = useState(false);
  const [deleteState, setDeleteState] = useState<DeleteState>("idle");
  const [deleteTarget, setDeleteTarget] = useState<GroupProjection | null>(null);
  const [deleteSubmissionActive, setDeleteSubmissionActive] = useState(false);
  const [totpCode, setTotpCode] = useState("");
  const mounted = useRef(true);
  const mutationActive = useRef(false);
  const deleteActive = useRef(false);
  const collectionRequest = useRef(0);
  const selectionRequest = useRef(0);
  const administrationEnded = useRef(false);
  const grantMutationTicket = useRef("");
  const workspaceIsMounted = (): boolean => mounted.current;

  const readFailed = useCallback(
    (error: unknown): void => {
      if (
        (error instanceof GroupSessionInvalidError ||
          error instanceof GroupAdministrationAccessDeniedError ||
          error instanceof MfaPolicySessionInvalidError) &&
        onAdministrationEnded !== undefined
      ) {
        if (!administrationEnded.current) {
          administrationEnded.current = true;
          onAdministrationEnded();
        }
      } else {
        setCollectionState("failed");
      }
    },
    [onAdministrationEnded],
  );

  const load = useCallback(
    (reconcile = false): void => {
      const request = collectionRequest.current + 1;
      collectionRequest.current = request;
      selectionRequest.current += 1;
      setCollectionState("loading");
      setSelected(null);
      void listGroups().then(
        (page) => {
          if (mounted.current && collectionRequest.current === request) {
            setGroups(page.items);
            setNextCursor(page.nextCursor);
            setCollectionState("ready");
            if (reconcile) {
              setRefreshRequired(false);
              setMutationState("idle");
              setDeleteState("idle");
              setDeleteTarget(null);
            }
          }
        },
        (error: unknown) => {
          if (
            mounted.current &&
            (collectionRequest.current === request ||
              error instanceof GroupSessionInvalidError ||
              error instanceof GroupAdministrationAccessDeniedError)
          )
            readFailed(error);
        },
      );
    },
    [readFailed],
  );

  useEffect(() => {
    mounted.current = true;
    void Promise.resolve().then(() => {
      load();
    });
    return () => {
      mounted.current = false;
      mutationActive.current = false;
      deleteActive.current = false;
      grantMutationTicket.current = "";
    };
  }, [load]);

  const select = (publicId: string): void => {
    const request = selectionRequest.current + 1;
    selectionRequest.current = request;
    void viewGroup(publicId).then(
      (group) => {
        if (mounted.current && selectionRequest.current === request) {
          setSelected(group);
          setEditName(group.name);
          setEditDescription(group.description ?? "");
        }
      },
      (error: unknown) => {
        if (
          mounted.current &&
          (selectionRequest.current === request ||
            error instanceof GroupSessionInvalidError ||
            error instanceof GroupAdministrationAccessDeniedError)
        )
          readFailed(error);
      },
    );
  };

  const loadMore = (): void => {
    if (nextCursor === null || collectionState !== "ready") return;
    const request = collectionRequest.current;
    setCollectionState("loading-more");
    void listGroups(nextCursor).then(
      (page) => {
        if (mounted.current && collectionRequest.current === request) {
          setGroups((current) => [...current, ...page.items]);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      (error: unknown) => {
        if (
          mounted.current &&
          (collectionRequest.current === request ||
            error instanceof GroupSessionInvalidError ||
            error instanceof GroupAdministrationAccessDeniedError)
        )
          readFailed(error);
      },
    );
  };

  const submitCreate = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (mutationActive.current || refreshRequired || createName === "") return;
    mutationActive.current = true;
    setMutationState("submitting");
    const description = createDescription === "" ? null : createDescription;
    void createGroup(createName, description)
      .then(
        () => {
          if (mounted.current) {
            setCreateName("");
            setCreateDescription("");
            setMutationState("idle");
            load();
          }
        },
        (error: unknown) => {
          if (mounted.current) {
            if (
              error instanceof GroupSessionInvalidError ||
              error instanceof GroupAdministrationAccessDeniedError
            ) {
              setMutationState("idle");
              readFailed(error);
              return;
            }
            const refused = error instanceof GroupMutationRefusedError;
            setMutationState(refused ? "refused" : "indeterminate");
            if (!refused) setRefreshRequired(true);
          }
        },
      )
      .finally(() => {
        mutationActive.current = false;
      });
  };

  const submitUpdate = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (mutationActive.current || refreshRequired || selected === null || editName === "") return;
    mutationActive.current = true;
    setMutationState("submitting");
    const description = editDescription === "" ? null : editDescription;
    void updateGroup(selected.publicId, editName, description)
      .then(
        (group) => {
          if (mounted.current) {
            setSelected(group);
            setMutationState("idle");
            load();
          }
        },
        (error: unknown) => {
          if (mounted.current) {
            if (
              error instanceof GroupSessionInvalidError ||
              error instanceof GroupAdministrationAccessDeniedError
            ) {
              setMutationState("idle");
              readFailed(error);
              return;
            }
            const refused = error instanceof GroupMutationRefusedError;
            setMutationState(refused ? "refused" : "indeterminate");
            if (!refused) setRefreshRequired(true);
          }
        },
      )
      .finally(() => {
        mutationActive.current = false;
      });
  };

  const beginDelete = (group: GroupProjection): void => {
    if (deleteActive.current || mutationActive.current || refreshRequired) return;
    setDeleteTarget(group);
    setDeleteState("confirming");
    setTotpCode("");
  };
  const confirmDelete = (): void => {
    if (deleteTarget !== null) setDeleteState("step-up");
  };
  const cancelDelete = (): void => {
    if (!deleteActive.current) {
      setDeleteTarget(null);
      setDeleteState("idle");
      setTotpCode("");
      grantMutationTicket.current = "";
    }
  };
  const submitDelete = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (
      deleteActive.current ||
      refreshRequired ||
      deleteTarget === null ||
      !TOTP_PATTERN.test(totpCode)
    )
      return;
    deleteActive.current = true;
    setDeleteSubmissionActive(true);
    const target = deleteTarget;
    const code = totpCode;
    setTotpCode("");
    setDeleteState("step-up");
    void (async () => {
      try {
        grantMutationTicket.current = await issueGrantMutationStepUp(code);
        if (!mounted.current) return;
        const ticket = grantMutationTicket.current;
        grantMutationTicket.current = "";
        setDeleteState("deleting");
        await deleteGroup(target.publicId, ticket);
        if (workspaceIsMounted()) {
          setDeleteTarget(null);
          setDeleteState("idle");
          load();
        }
      } catch (error: unknown) {
        grantMutationTicket.current = "";
        if (mounted.current) {
          if (
            error instanceof MfaPolicyGrantMutationAccessDeniedError ||
            error instanceof MfaPolicySessionInvalidError ||
            error instanceof GroupSessionInvalidError ||
            error instanceof GroupAdministrationAccessDeniedError
          ) {
            setDeleteTarget(null);
            setDeleteState("idle");
            readFailed(error);
          } else {
            const refused =
              error instanceof GroupMutationRefusedError || error instanceof MfaPolicyRefusedError;
            setDeleteState(refused ? "refused" : "indeterminate");
            if (!refused) setRefreshRequired(true);
          }
        }
      } finally {
        deleteActive.current = false;
        if (workspaceIsMounted()) setDeleteSubmissionActive(false);
      }
    })();
  };

  return (
    <section className="accounts groups" aria-labelledby="groups-title">
      <header className="accounts__header">
        <div>
          <h2 id="groups-title" className="accounts__title">
            Groups
          </h2>
          <p className="accounts__summary">Administration groups</p>
        </div>
        <button
          type="button"
          onClick={() => {
            load(true);
          }}
          disabled={
            collectionState === "loading" ||
            mutationState === "submitting" ||
            deleteSubmissionActive
          }
        >
          Refresh
        </button>
      </header>

      {collectionState === "failed" ? <p role="alert">Groups are unavailable.</p> : null}
      <div className="accounts__layout">
        <div className="accounts__collection">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Description</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {groups.map((group) => (
                <tr key={group.publicId}>
                  <td>{group.name}</td>
                  <td>{group.description ?? "None"}</td>
                  <td>
                    <button
                      type="button"
                      onClick={() => {
                        select(group.publicId);
                      }}
                    >
                      View
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {nextCursor !== null ? (
            <button type="button" onClick={loadMore} disabled={collectionState !== "ready"}>
              Load more
            </button>
          ) : null}
        </div>

        <aside className="accounts__detail" aria-label="Group detail">
          {selected === null ? (
            <p>Select a Group.</p>
          ) : (
            <>
              <h3>{selected.name}</h3>
              <p className="accounts__identifier">{selected.publicId}</p>
              <form onSubmit={submitUpdate}>
                <label>
                  Name
                  <input
                    value={editName}
                    onChange={(event) => {
                      setEditName(event.currentTarget.value);
                    }}
                    required
                    maxLength={256}
                  />
                </label>
                <label>
                  Description
                  <textarea
                    value={editDescription}
                    onChange={(event) => {
                      setEditDescription(event.currentTarget.value);
                    }}
                    maxLength={1024}
                  />
                </label>
                <button type="submit" disabled={mutationState === "submitting" || refreshRequired}>
                  Update
                </button>
                <button
                  type="button"
                  onClick={() => {
                    beginDelete(selected);
                  }}
                  disabled={refreshRequired}
                >
                  Delete
                </button>
              </form>
              <GroupAssociations
                key={selected.publicId}
                groupPublicId={selected.publicId}
                mutationsLocked={refreshRequired}
                {...(onAdministrationEnded === undefined ? {} : { onAdministrationEnded })}
                onMutationIndeterminate={() => {
                  setRefreshRequired(true);
                }}
              />
            </>
          )}
        </aside>
      </div>

      <form className="accounts__create" onSubmit={submitCreate}>
        <h3>Create Group</h3>
        <label>
          Name
          <input
            value={createName}
            onChange={(event) => {
              setCreateName(event.currentTarget.value);
            }}
            required
            maxLength={256}
          />
        </label>
        <label>
          Description
          <textarea
            value={createDescription}
            onChange={(event) => {
              setCreateDescription(event.currentTarget.value);
            }}
            maxLength={1024}
          />
        </label>
        <button type="submit" disabled={mutationState === "submitting" || refreshRequired}>
          Create
        </button>
      </form>
      {mutationState === "refused" ? <p role="alert">The Group was not changed.</p> : null}
      {mutationState === "indeterminate" ? (
        <p role="alert">
          The Group outcome is unknown. Refresh before taking another Group action.
        </p>
      ) : null}

      {deleteTarget !== null && deleteState === "confirming" ? (
        <section className="accounts__confirmation" aria-label="Confirm Group deletion">
          <h3>Delete {deleteTarget.name}?</h3>
          <p>Only an empty Group can be deleted.</p>
          <button type="button" onClick={confirmDelete}>
            Confirm
          </button>
          <button type="button" onClick={cancelDelete}>
            Cancel
          </button>
        </section>
      ) : null}
      {deleteTarget !== null && (deleteState === "step-up" || deleteState === "deleting") ? (
        <form className="accounts__confirmation" onSubmit={submitDelete}>
          <h3>Verify deletion</h3>
          <label>
            TOTP code
            <input
              inputMode="numeric"
              autoComplete="one-time-code"
              value={totpCode}
              onChange={(event) => {
                setTotpCode(event.currentTarget.value);
              }}
              pattern="[0-9]{6}"
              maxLength={6}
              disabled={deleteSubmissionActive}
            />
          </label>
          <button type="submit" disabled={!TOTP_PATTERN.test(totpCode) || deleteSubmissionActive}>
            Delete Group
          </button>
          <button type="button" onClick={cancelDelete} disabled={deleteSubmissionActive}>
            Cancel
          </button>
        </form>
      ) : null}
      {deleteState === "refused" ? <p role="alert">The Group was not deleted.</p> : null}
      {deleteState === "indeterminate" ? (
        <p role="alert">
          The Group deletion outcome is unknown. Refresh before taking another Group action.
        </p>
      ) : null}
    </section>
  );
}
