import { useEffect, useRef, useState, type JSX, type SyntheticEvent } from "react";

import {
  GroupMutationRefusedError,
  createGroup,
  deleteGroup,
  listGroups,
  updateGroup,
  viewGroup,
  type GroupProjection,
} from "../api/weavelit-administration-groups";
import { issueGrantMutationStepUp } from "../api/weavelit-mfa-policy";

type CollectionState = "loading" | "ready" | "loading-more" | "failed";
type MutationState = "idle" | "submitting" | "refused" | "indeterminate";
type DeleteState = "idle" | "confirming" | "step-up" | "deleting" | "refused" | "indeterminate";

const TOTP_PATTERN = /^[0-9]{6}$/;

export function GroupsWorkspace(): JSX.Element {
  const [groups, setGroups] = useState<readonly GroupProjection[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [collectionState, setCollectionState] = useState<CollectionState>("loading");
  const [selected, setSelected] = useState<GroupProjection | null>(null);
  const [createName, setCreateName] = useState("");
  const [createDescription, setCreateDescription] = useState("");
  const [editName, setEditName] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [mutationState, setMutationState] = useState<MutationState>("idle");
  const [deleteState, setDeleteState] = useState<DeleteState>("idle");
  const [deleteTarget, setDeleteTarget] = useState<GroupProjection | null>(null);
  const [deleteSubmissionActive, setDeleteSubmissionActive] = useState(false);
  const [totpCode, setTotpCode] = useState("");
  const mounted = useRef(true);
  const mutationActive = useRef(false);
  const deleteActive = useRef(false);
  const grantMutationTicket = useRef("");
  const workspaceIsMounted = (): boolean => mounted.current;

  const load = (): void => {
    setCollectionState("loading");
    setSelected(null);
    void listGroups().then(
      (page) => {
        if (mounted.current) {
          setGroups(page.items);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      () => {
        if (mounted.current) setCollectionState("failed");
      },
    );
  };

  useEffect(() => {
    mounted.current = true;
    void Promise.resolve().then(load);
    return () => {
      mounted.current = false;
      mutationActive.current = false;
      deleteActive.current = false;
      grantMutationTicket.current = "";
    };
  }, []);

  const select = (publicId: string): void => {
    void viewGroup(publicId).then(
      (group) => {
        if (mounted.current) {
          setSelected(group);
          setEditName(group.name);
          setEditDescription(group.description ?? "");
        }
      },
      () => {
        if (mounted.current) setCollectionState("failed");
      },
    );
  };

  const loadMore = (): void => {
    if (nextCursor === null || collectionState !== "ready") return;
    setCollectionState("loading-more");
    void listGroups(nextCursor).then(
      (page) => {
        if (mounted.current) {
          setGroups((current) => [...current, ...page.items]);
          setNextCursor(page.nextCursor);
          setCollectionState("ready");
        }
      },
      () => {
        if (mounted.current) setCollectionState("failed");
      },
    );
  };

  const submitCreate = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (mutationActive.current || createName === "") return;
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
          if (mounted.current)
            setMutationState(
              error instanceof GroupMutationRefusedError ? "refused" : "indeterminate",
            );
        },
      )
      .finally(() => {
        mutationActive.current = false;
      });
  };

  const submitUpdate = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (mutationActive.current || selected === null || editName === "") return;
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
          if (mounted.current)
            setMutationState(
              error instanceof GroupMutationRefusedError ? "refused" : "indeterminate",
            );
        },
      )
      .finally(() => {
        mutationActive.current = false;
      });
  };

  const beginDelete = (group: GroupProjection): void => {
    if (deleteActive.current || mutationActive.current) return;
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
    if (deleteActive.current || deleteTarget === null || !TOTP_PATTERN.test(totpCode)) return;
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
        if (mounted.current)
          setDeleteState(error instanceof GroupMutationRefusedError ? "refused" : "indeterminate");
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
        <button type="button" onClick={load} disabled={collectionState === "loading"}>
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
                <button type="submit" disabled={mutationState === "submitting"}>
                  Update
                </button>
                <button
                  type="button"
                  onClick={() => {
                    beginDelete(selected);
                  }}
                >
                  Delete
                </button>
              </form>
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
        <button type="submit" disabled={mutationState === "submitting"}>
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
