import { useCallback, useEffect, useRef, useState, type JSX, type SyntheticEvent } from "react";

import {
  AccountsSessionExpiredError,
  listAccounts,
  type AccountProjection,
} from "../api/weavelit-administration-accounts";
import {
  GroupAdministrationAccessDeniedError,
  GroupMutationRefusedError,
  GroupSessionInvalidError,
  LastAdministratorRefusedError,
  changeGroupGrant,
  changeGroupMember,
  listGroupGrants,
  listGroupMembers,
  loadAdministrationCatalog,
  type AdministrationCatalog,
  type GroupGrant,
} from "../api/weavelit-administration-groups";
import {
  MfaPolicyRefusedError,
  MfaPolicySessionInvalidError,
  issueGrantMutationStepUp,
} from "../api/weavelit-mfa-policy";

type Phase = "idle" | "confirm" | "verify" | "busy" | "refused" | "last" | "unknown";
type Action =
  | { readonly member: AccountProjection; readonly present: boolean }
  | { readonly grant: GroupGrant; readonly present: boolean };
interface Data {
  readonly members: readonly AccountProjection[];
  readonly memberCursor: string | null;
  readonly accounts: readonly AccountProjection[];
  readonly accountCursor: string | null;
  readonly grants: readonly GroupGrant[];
  readonly grantCursor: string | null;
  readonly catalog: AdministrationCatalog;
}

const CODE = /^[0-9]{6}$/;

function grantLabel(grant: GroupGrant): string {
  if (grant.type === "server_administration") return "Server Administration Permission";
  const kind =
    grant.type === "client_module"
      ? "Client Module"
      : grant.type === "service_module"
        ? "Service Module"
        : "Operation";
  return `${kind}: ${grant.value}`;
}

function values(catalog: AdministrationCatalog, kind: GroupGrant["type"]): readonly string[] {
  if (kind === "client_module") return catalog.clientModules;
  if (kind === "service_module") return catalog.serviceModules;
  if (kind === "operation") return catalog.operations;
  return [];
}

async function loadData(group: string): Promise<Data> {
  const [members, grants, accounts, catalog] = await Promise.all([
    listGroupMembers(group),
    listGroupGrants(group),
    listAccounts(),
    loadAdministrationCatalog(),
  ]);
  return {
    members: members.items,
    memberCursor: members.nextCursor,
    grants: grants.items,
    grantCursor: grants.nextCursor,
    accounts: accounts.items,
    accountCursor: accounts.nextCursor,
    catalog,
  };
}

export function GroupAssociations({
  groupPublicId,
  mutationsLocked = false,
  onAdministrationEnded,
  onMutationIndeterminate,
}: {
  readonly groupPublicId: string;
  readonly mutationsLocked?: boolean;
  readonly onAdministrationEnded?: () => void;
  readonly onMutationIndeterminate?: () => void;
}): JSX.Element {
  const [data, setData] = useState<Data | null>(null);
  const [failed, setFailed] = useState(false);
  const [accountId, setAccountId] = useState("");
  const [grantKind, setGrantKind] = useState<GroupGrant["type"]>("server_administration");
  const [grantValue, setGrantValue] = useState("");
  const [action, setAction] = useState<Action | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [code, setCode] = useState("");
  const [membersPending, setMembersPending] = useState(false);
  const [grantsPending, setGrantsPending] = useState(false);
  const [accountsPending, setAccountsPending] = useState(false);
  const mounted = useRef(true);
  const active = useRef(false);
  const administrationEnded = useRef(false);
  const ticket = useRef("");
  const mutationLocked = mutationsLocked || phase === "unknown";
  const workspaceIsMounted = (): boolean => mounted.current;

  const readFailed = useCallback(
    (error: unknown): void => {
      if (
        (error instanceof GroupSessionInvalidError ||
          error instanceof GroupAdministrationAccessDeniedError ||
          error instanceof AccountsSessionExpiredError ||
          error instanceof MfaPolicySessionInvalidError) &&
        onAdministrationEnded !== undefined
      ) {
        if (!administrationEnded.current) {
          administrationEnded.current = true;
          onAdministrationEnded();
        }
      } else {
        setFailed(true);
      }
    },
    [onAdministrationEnded],
  );

  useEffect(() => {
    mounted.current = true;
    void loadData(groupPublicId).then(
      (loaded) => {
        if (mounted.current) setData(loaded);
      },
      (error: unknown) => {
        if (mounted.current) readFailed(error);
      },
    );
    return () => {
      mounted.current = false;
      active.current = false;
      ticket.current = "";
      setMembersPending(false);
      setGrantsPending(false);
      setAccountsPending(false);
    };
  }, [groupPublicId, readFailed]);

  const begin = (next: Action): void => {
    if (active.current || mutationLocked || action !== null) return;
    setAction(next);
    setCode("");
    setPhase(next.present ? "verify" : "confirm");
  };

  const cancel = (): void => {
    if (active.current) return;
    ticket.current = "";
    setAction(null);
    setCode("");
    setPhase("idle");
  };

  const submit = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (active.current || mutationLocked || action === null || !CODE.test(code)) return;
    active.current = true;
    const requested = action;
    const submittedCode = code;
    setCode("");
    setPhase("busy");
    void (async () => {
      try {
        ticket.current = await issueGrantMutationStepUp(submittedCode);
        const proof = ticket.current;
        ticket.current = "";
        if ("member" in requested) {
          await changeGroupMember(
            groupPublicId,
            requested.member.publicId,
            requested.present,
            proof,
          );
        } else {
          await changeGroupGrant(groupPublicId, requested.grant, requested.present, proof);
        }
        if (!workspaceIsMounted()) return;
        setAction(null);
        setAccountId("");
        setGrantValue("");
        setPhase("idle");
      } catch (error: unknown) {
        ticket.current = "";
        if (!mounted.current) return;
        setAction(null);
        if (
          error instanceof MfaPolicySessionInvalidError ||
          error instanceof GroupSessionInvalidError ||
          error instanceof GroupAdministrationAccessDeniedError
        ) {
          setPhase("idle");
          readFailed(error);
        } else if (error instanceof LastAdministratorRefusedError) setPhase("last");
        else if (
          error instanceof GroupMutationRefusedError ||
          error instanceof MfaPolicyRefusedError
        )
          setPhase("refused");
        else {
          setPhase("unknown");
          onMutationIndeterminate?.();
        }
        active.current = false;
        return;
      }

      try {
        const [members, grants] = await Promise.all([
          listGroupMembers(groupPublicId),
          listGroupGrants(groupPublicId),
        ]);
        if (!mounted.current) return;
        setData((current) =>
          current === null
            ? current
            : {
                ...current,
                members: members.items,
                memberCursor: members.nextCursor,
                grants: grants.items,
                grantCursor: grants.nextCursor,
              },
        );
      } catch (error: unknown) {
        if (workspaceIsMounted()) readFailed(error);
      } finally {
        active.current = false;
      }
    })();
  };

  const moreMembers = (): void => {
    const cursor = data?.memberCursor;
    if (cursor === null || cursor === undefined || membersPending) return;
    setMembersPending(true);
    void listGroupMembers(groupPublicId, cursor)
      .then(
        (page) => {
          if (mounted.current)
            setData((current) =>
              current === null
                ? current
                : {
                    ...current,
                    members: [...current.members, ...page.items],
                    memberCursor: page.nextCursor,
                  },
            );
        },
        (error: unknown) => {
          if (mounted.current) readFailed(error);
        },
      )
      .finally(() => {
        if (mounted.current) setMembersPending(false);
      });
  };

  const moreGrants = (): void => {
    const cursor = data?.grantCursor;
    if (cursor === null || cursor === undefined || grantsPending) return;
    setGrantsPending(true);
    void listGroupGrants(groupPublicId, cursor)
      .then(
        (page) => {
          if (mounted.current)
            setData((current) =>
              current === null
                ? current
                : {
                    ...current,
                    grants: [...current.grants, ...page.items],
                    grantCursor: page.nextCursor,
                  },
            );
        },
        (error: unknown) => {
          if (mounted.current) readFailed(error);
        },
      )
      .finally(() => {
        if (mounted.current) setGrantsPending(false);
      });
  };

  const moreAccounts = (): void => {
    const cursor = data?.accountCursor;
    if (cursor === null || cursor === undefined || accountsPending) return;
    setAccountsPending(true);
    void listAccounts(cursor)
      .then(
        (page) => {
          if (mounted.current)
            setData((current) =>
              current === null
                ? current
                : {
                    ...current,
                    accounts: [...current.accounts, ...page.items],
                    accountCursor: page.nextCursor,
                  },
            );
        },
        (error: unknown) => {
          if (mounted.current) readFailed(error);
        },
      )
      .finally(() => {
        if (mounted.current) setAccountsPending(false);
      });
  };

  if (failed) return <p role="alert">Group access is unavailable.</p>;
  if (data === null) return <p>Loading Group access...</p>;

  const member = data.accounts.find((account) => account.publicId === accountId) ?? null;
  const memberIds = new Set(data.members.map((account) => account.publicId));
  const grant: GroupGrant | null =
    grantKind === "server_administration"
      ? { type: grantKind }
      : grantValue === ""
        ? null
        : { type: grantKind, value: grantValue };
  const granted =
    grant !== null && data.grants.some((current) => grantLabel(current) === grantLabel(grant));

  return (
    <section className="groups__associations" aria-labelledby="group-associations-title">
      <h4 id="group-associations-title">Access</h4>
      <section className="groups__association-section" aria-labelledby="group-members-title">
        <h5 id="group-members-title">Members</h5>
        {data.members.length === 0 ? (
          <p>No members.</p>
        ) : (
          <ul className="groups__association-list">
            {data.members.map((account) => (
              <li key={account.publicId}>
                <span>
                  {account.displayName ?? account.username} <small>{account.username}</small>
                </span>
                <button
                  type="button"
                  disabled={mutationLocked}
                  onClick={() => {
                    begin({ member: account, present: false });
                  }}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        {data.memberCursor === null ? null : (
          <button type="button" disabled={membersPending} onClick={moreMembers}>
            Load more members
          </button>
        )}
        <label>
          Add member
          <select
            value={accountId}
            onChange={(event) => {
              setAccountId(event.currentTarget.value);
            }}
          >
            <option value="">Select an Account</option>
            {data.accounts
              .filter((account) => !memberIds.has(account.publicId))
              .map((account) => (
                <option key={account.publicId} value={account.publicId}>
                  {account.username}
                </option>
              ))}
          </select>
        </label>
        <button
          type="button"
          disabled={member === null || mutationLocked}
          onClick={() => {
            if (member !== null) begin({ member, present: true });
          }}
        >
          Add member
        </button>
        {data.accountCursor === null ? null : (
          <button type="button" disabled={accountsPending} onClick={moreAccounts}>
            Load more Accounts
          </button>
        )}
      </section>

      <section className="groups__association-section" aria-labelledby="group-grants-title">
        <h5 id="group-grants-title">Direct grants</h5>
        {data.grants.length === 0 ? (
          <p>No direct grants.</p>
        ) : (
          <ul className="groups__association-list">
            {data.grants.map((current) => (
              <li key={grantLabel(current)}>
                <span>{grantLabel(current)}</span>
                <button
                  type="button"
                  disabled={mutationLocked}
                  onClick={() => {
                    begin({ grant: current, present: false });
                  }}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        {data.grantCursor === null ? null : (
          <button type="button" disabled={grantsPending} onClick={moreGrants}>
            Load more grants
          </button>
        )}
        <label>
          Grant type
          <select
            value={grantKind}
            onChange={(event) => {
              setGrantKind(event.currentTarget.value as GroupGrant["type"]);
              setGrantValue("");
            }}
          >
            <option value="server_administration">Server Administration Permission</option>
            {data.catalog.clientModules.length > 0 ? (
              <option value="client_module">Client Module</option>
            ) : null}
            {data.catalog.serviceModules.length > 0 ? (
              <option value="service_module">Service Module</option>
            ) : null}
            {data.catalog.operations.length > 0 ? (
              <option value="operation">Operation</option>
            ) : null}
          </select>
        </label>
        {grantKind === "server_administration" ? null : (
          <label>
            Grant target
            <select
              value={grantValue}
              onChange={(event) => {
                setGrantValue(event.currentTarget.value);
              }}
            >
              <option value="">Select a target</option>
              {values(data.catalog, grantKind).map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </label>
        )}
        <button
          type="button"
          disabled={grant === null || granted || mutationLocked}
          onClick={() => {
            if (grant !== null) begin({ grant, present: true });
          }}
        >
          Add grant
        </button>
      </section>

      {action !== null && phase === "confirm" ? (
        <section className="accounts__confirmation" aria-label="Confirm Group access removal">
          <h5>Confirm removal</h5>
          <p>Remove {"member" in action ? action.member.username : grantLabel(action.grant)}?</p>
          <button
            type="button"
            disabled={mutationLocked}
            onClick={() => {
              setPhase("verify");
            }}
          >
            Continue
          </button>
          <button type="button" onClick={cancel}>
            Cancel
          </button>
        </section>
      ) : null}
      {action !== null && (phase === "verify" || phase === "busy") ? (
        <form className="accounts__confirmation" onSubmit={submit}>
          <h5>Verify Group access change</h5>
          <label>
            TOTP code
            <input
              inputMode="numeric"
              autoComplete="one-time-code"
              value={code}
              onChange={(event) => {
                setCode(event.currentTarget.value);
              }}
              pattern="[0-9]{6}"
              maxLength={6}
              disabled={phase === "busy"}
            />
          </label>
          <button type="submit" disabled={!CODE.test(code) || phase === "busy" || mutationLocked}>
            Apply change
          </button>
          <button type="button" onClick={cancel} disabled={phase === "busy"}>
            Cancel
          </button>
        </form>
      ) : null}
      {phase === "last" ? (
        <p role="alert">Cannot remove the last Server Administration Permission grant.</p>
      ) : null}
      {phase === "refused" ? <p role="alert">Group access was not changed.</p> : null}
      {phase === "unknown" ? (
        <p role="alert">
          The Group access outcome is unknown. Refresh before taking another access action.
        </p>
      ) : null}
    </section>
  );
}
