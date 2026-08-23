import { useCallback, useEffect, useRef, useState, type JSX, type SyntheticEvent } from "react";

import {
  ConfigurationAdministrationAccessDeniedError,
  ConfigurationConflictError,
  ConfigurationRefusedError,
  ConfigurationSessionInvalidError,
  applyTotpEnablement,
  changeLogConfiguration,
  listLogConfigurations,
  previewTotpEnablement,
  viewLogConfiguration,
  type LogConfiguration,
  type LogSetting,
  type LogType,
} from "../api/weavelit-administration-configuration";
import { probeSession } from "../api/weavelit-authentication";

type CollectionState = "loading" | "ready" | "loading-more" | "failed";
type TotpState =
  | "idle"
  | "previewing"
  | "preview-failed"
  | "review"
  | "applying"
  | "refused"
  | "conflict"
  | "indeterminate";
type LogChangeState = "idle" | "submitting" | "refused" | "conflict" | "indeterminate";

interface TotpReview {
  readonly currentEnabled: boolean;
  readonly desiredEnabled: boolean;
  readonly affectedUsers: number;
}

export interface ConfigurationWorkspaceProps {
  readonly onAdministrationEnded?: () => void;
}

export function ConfigurationWorkspace({
  onAdministrationEnded,
}: ConfigurationWorkspaceProps = {}): JSX.Element {
  const [configurations, setConfigurations] = useState<readonly LogConfiguration[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [collectionState, setCollectionState] = useState<CollectionState>("loading");
  const [selected, setSelected] = useState<LogConfiguration | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [settings, setSettings] = useState<readonly LogSetting[]>([]);
  const [systemAssignment, setSystemAssignment] = useState("");
  const [auditAssignment, setAuditAssignment] = useState("");
  const [logChangeState, setLogChangeState] = useState<LogChangeState>("idle");
  const [totpState, setTotpState] = useState<TotpState>("idle");
  const [totpReview, setTotpReview] = useState<TotpReview | null>(null);
  const mounted = useRef(true);
  const logChangeActive = useRef(false);
  const totpActive = useRef(false);
  const totpPreview = useRef("");
  const selectionRequest = useRef(0);
  const collectionRequest = useRef(0);
  const administrationEnded = useRef(false);

  const assignmentFor = (items: readonly LogConfiguration[], logType: LogType): string =>
    items.find((configuration) => configuration.assignedLogTypes.includes(logType))
      ?.configurationName ?? "";

  const endAdministration = useCallback((): boolean => {
    if (onAdministrationEnded === undefined) return false;
    if (!administrationEnded.current) {
      administrationEnded.current = true;
      onAdministrationEnded();
    }
    return true;
  }, [onAdministrationEnded]);

  const reconcileAdministrationLoss = useCallback(
    (error: unknown): boolean => {
      if (
        !(error instanceof ConfigurationSessionInvalidError) &&
        !(error instanceof ConfigurationAdministrationAccessDeniedError)
      )
        return false;
      return endAdministration();
    },
    [endAdministration],
  );

  const adoptCollection = useCallback(
    (items: readonly LogConfiguration[], cursor: string | null): void => {
      setConfigurations(items);
      setNextCursor(cursor);
      setCollectionState("ready");
      setSystemAssignment(assignmentFor(items, "system"));
      setAuditAssignment(assignmentFor(items, "audit"));
    },
    [],
  );

  const load = useCallback((): void => {
    if (logChangeActive.current || totpActive.current) return;
    const request = ++collectionRequest.current;
    selectionRequest.current += 1;
    setCollectionState("loading");
    void listLogConfigurations().then(
      (page) => {
        if (mounted.current && request === collectionRequest.current) {
          totpPreview.current = "";
          setSelected(null);
          setLogChangeState("idle");
          setTotpReview(null);
          setTotpState("idle");
          adoptCollection(page.items, page.nextCursor);
        }
      },
      (error: unknown) => {
        if (!mounted.current) return;
        if (reconcileAdministrationLoss(error)) return;
        if (request === collectionRequest.current) setCollectionState("failed");
      },
    );
  }, [adoptCollection, reconcileAdministrationLoss]);

  useEffect(() => {
    mounted.current = true;
    void Promise.resolve().then(load);
    return () => {
      mounted.current = false;
      logChangeActive.current = false;
      totpActive.current = false;
      totpPreview.current = "";
    };
  }, [load]);

  const beginTotpChange = (desired: boolean): void => {
    if (totpActive.current) return;
    totpActive.current = true;
    totpPreview.current = "";
    setTotpReview(null);
    setTotpState("previewing");
    void previewTotpEnablement(desired)
      .then(
        (preview) => {
          if (mounted.current) {
            totpPreview.current = preview.preview;
            setTotpReview({
              currentEnabled: preview.currentEnabled,
              desiredEnabled: preview.desiredEnabled,
              affectedUsers: preview.affectedUsers,
            });
            setTotpState("review");
          }
        },
        (error: unknown) => {
          if (!mounted.current) return;
          if (reconcileAdministrationLoss(error)) setTotpState("idle");
          else setTotpState("preview-failed");
        },
      )
      .finally(() => {
        totpActive.current = false;
      });
  };

  const cancelTotpChange = (): void => {
    if (totpActive.current) return;
    totpPreview.current = "";
    setTotpReview(null);
    setTotpState("idle");
  };

  const applyTotpChange = (): void => {
    if (totpActive.current || totpReview === null || totpPreview.current === "") return;
    totpActive.current = true;
    const credential = totpPreview.current;
    const desired = totpReview.desiredEnabled;
    totpPreview.current = "";
    setTotpState("applying");
    void applyTotpEnablement(desired, credential)
      .then(async () => {
        const probe = await probeSession();
        if (!mounted.current) return;
        if (probe.kind === "unauthenticated") {
          endAdministration();
          return;
        }
        if (probe.kind === "authenticated") {
          setTotpReview(null);
          setTotpState("idle");
          return;
        }
        setTotpState("indeterminate");
      })
      .catch((error: unknown) => {
        if (!mounted.current) return;
        if (reconcileAdministrationLoss(error)) return;
        if (error instanceof ConfigurationConflictError) setTotpState("conflict");
        else if (error instanceof ConfigurationRefusedError) setTotpState("refused");
        else setTotpState("indeterminate");
      })
      .finally(() => {
        totpActive.current = false;
      });
  };

  const loadMore = (): void => {
    if (logChangeActive.current || nextCursor === null || collectionState !== "ready") return;
    const request = collectionRequest.current;
    setCollectionState("loading-more");
    void listLogConfigurations(nextCursor).then(
      (page) => {
        if (mounted.current && request === collectionRequest.current) {
          const items = [...configurations, ...page.items];
          adoptCollection(items, page.nextCursor);
        }
      },
      (error: unknown) => {
        if (!mounted.current) return;
        if (reconcileAdministrationLoss(error)) return;
        if (request === collectionRequest.current) setCollectionState("failed");
      },
    );
  };

  const selectConfiguration = (configurationName: string): void => {
    if (logChangeState === "conflict" || logChangeState === "indeterminate") return;
    const request = ++selectionRequest.current;
    void viewLogConfiguration(configurationName).then(
      (configuration) => {
        if (mounted.current && request === selectionRequest.current) {
          setSelected(configuration);
          setEnabled(configuration.enabled);
          setSettings(configuration.settings);
          setLogChangeState("idle");
        }
      },
      (error: unknown) => {
        if (!mounted.current) return;
        if (reconcileAdministrationLoss(error)) return;
        else if (request === selectionRequest.current) setCollectionState("failed");
      },
    );
  };

  const submitLogChange = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (
      logChangeActive.current ||
      collectionState !== "ready" ||
      selected === null ||
      systemAssignment === "" ||
      auditAssignment === ""
    )
      return;
    logChangeActive.current = true;
    setLogChangeState("submitting");
    void changeLogConfiguration({
      configurationName: selected.configurationName,
      enabled,
      settings,
      assignments: [
        { logType: "system", configurationName: systemAssignment },
        { logType: "audit", configurationName: auditAssignment },
      ],
    })
      .then(
        (configuration) => {
          if (mounted.current) {
            collectionRequest.current += 1;
            setCollectionState("ready");
            setSelected(configuration);
            setConfigurations((current) =>
              current.map((item) =>
                item.configurationName === configuration.configurationName ? configuration : item,
              ),
            );
            setLogChangeState("idle");
          }
        },
        (error: unknown) => {
          if (!mounted.current) return;
          if (reconcileAdministrationLoss(error)) return;
          if (error instanceof ConfigurationConflictError) setLogChangeState("conflict");
          else if (error instanceof ConfigurationRefusedError) setLogChangeState("refused");
          else setLogChangeState("indeterminate");
        },
      )
      .finally(() => {
        logChangeActive.current = false;
      });
  };

  const totpSubmissionActive = totpState === "previewing" || totpState === "applying";
  const totpChangeLocked = totpState === "indeterminate";
  const logChangeLocked = logChangeState === "conflict" || logChangeState === "indeterminate";
  const logChangeControlsLocked =
    collectionState !== "ready" || logChangeState === "submitting" || logChangeLocked;

  return (
    <section className="accounts configuration" aria-labelledby="configuration-title">
      <header className="accounts__header">
        <div>
          <h2 id="configuration-title" className="accounts__title">
            Configuration
          </h2>
          <p className="accounts__summary">MFA and Log Modules</p>
        </div>
        <button
          type="button"
          onClick={load}
          disabled={
            collectionState === "loading" || logChangeState === "submitting" || totpSubmissionActive
          }
        >
          Refresh
        </button>
      </header>

      <section className="configuration__totp" aria-labelledby="totp-enablement-title">
        <h3 id="totp-enablement-title">TOTP enablement</h3>
        <div className="configuration__actions">
          <button
            type="button"
            onClick={() => {
              beginTotpChange(true);
            }}
            disabled={totpSubmissionActive || totpChangeLocked}
          >
            Review enablement
          </button>
          <button
            type="button"
            onClick={() => {
              beginTotpChange(false);
            }}
            disabled={totpSubmissionActive || totpChangeLocked}
          >
            Review disablement
          </button>
        </div>
        {totpState === "previewing" ? <p role="status">Loading TOTP preview.</p> : null}
        {totpState === "preview-failed" ? (
          <p role="alert">TOTP preview is unavailable. Try again.</p>
        ) : null}
        {totpState === "review" && totpReview !== null ? (
          <div className="configuration__review">
            <p>
              TOTP is currently {totpReview.currentEnabled ? "enabled" : "disabled"}. This change
              affects {totpReview.affectedUsers} enrolled account
              {totpReview.affectedUsers === 1 ? "" : "s"}.
            </p>
            {!totpReview.desiredEnabled && totpReview.affectedUsers > 0 ? (
              <p>Disabling TOTP ends sessions for enrolled accounts.</p>
            ) : null}
            <button type="button" onClick={applyTotpChange}>
              Apply {totpReview.desiredEnabled ? "enablement" : "disablement"}
            </button>
            <button type="button" onClick={cancelTotpChange}>
              Cancel
            </button>
          </div>
        ) : null}
        {totpState === "applying" ? <p role="status">Applying TOTP enablement.</p> : null}
        {totpState === "refused" ? <p role="alert">TOTP enablement was not changed.</p> : null}
        {totpState === "conflict" ? (
          <p role="alert">TOTP enrollment changed. Review the change again.</p>
        ) : null}
        {totpState === "indeterminate" ? (
          <p role="alert">The TOTP enablement outcome is unknown. Refresh before another change.</p>
        ) : null}
      </section>

      {collectionState === "failed" ? (
        <p role="alert">Log configurations are unavailable.</p>
      ) : null}
      <div className="accounts__layout">
        <div className="accounts__collection">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Module</th>
                <th>State</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {configurations.map((configuration) => (
                <tr key={configuration.configurationName}>
                  <td>{configuration.configurationName}</td>
                  <td>{configuration.module}</td>
                  <td>{configuration.enabled ? "Enabled" : "Disabled"}</td>
                  <td>
                    <button
                      type="button"
                      onClick={() => {
                        selectConfiguration(configuration.configurationName);
                      }}
                      disabled={logChangeLocked}
                    >
                      View
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {nextCursor !== null ? (
            <button
              type="button"
              onClick={loadMore}
              disabled={
                collectionState !== "ready" || logChangeState === "submitting" || logChangeLocked
              }
            >
              Load more
            </button>
          ) : null}
        </div>

        <aside className="accounts__detail" aria-label="Log configuration detail">
          {selected === null ? (
            <p>Select a Log configuration.</p>
          ) : (
            <form onSubmit={submitLogChange}>
              <h3>{selected.configurationName}</h3>
              <p>{selected.module}</p>
              <label>
                <input
                  type="checkbox"
                  checked={enabled}
                  onChange={(event) => {
                    setEnabled(event.currentTarget.checked);
                  }}
                  disabled={logChangeControlsLocked}
                />
                Enabled
              </label>
              {settings.map((setting, index) => (
                <label key={setting.key}>
                  {setting.key}
                  <input
                    value={setting.value}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setSettings((current) =>
                        current.map((item, itemIndex) =>
                          itemIndex === index ? { key: item.key, value } : item,
                        ),
                      );
                    }}
                    maxLength={4096}
                    required
                    disabled={logChangeControlsLocked}
                  />
                </label>
              ))}
              <label>
                System Logs
                <select
                  value={systemAssignment}
                  onChange={(event) => {
                    setSystemAssignment(event.currentTarget.value);
                  }}
                  required
                  disabled={logChangeControlsLocked}
                >
                  <option value="">Select configuration</option>
                  {configurations.map((configuration) => (
                    <option
                      key={configuration.configurationName}
                      value={configuration.configurationName}
                    >
                      {configuration.configurationName}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Audit Logs
                <select
                  value={auditAssignment}
                  onChange={(event) => {
                    setAuditAssignment(event.currentTarget.value);
                  }}
                  required
                  disabled={logChangeControlsLocked}
                >
                  <option value="">Select configuration</option>
                  {configurations.map((configuration) => (
                    <option
                      key={configuration.configurationName}
                      value={configuration.configurationName}
                    >
                      {configuration.configurationName}
                    </option>
                  ))}
                </select>
              </label>
              <button type="submit" disabled={logChangeControlsLocked}>
                Save configuration
              </button>
              {logChangeState === "refused" ? (
                <p role="alert">The Log configuration was not changed.</p>
              ) : null}
              {logChangeState === "conflict" ? (
                <p role="alert">The Log configuration changed. Refresh before another change.</p>
              ) : null}
              {logChangeState === "indeterminate" ? (
                <p role="alert">
                  The Log configuration outcome is unknown. Refresh before another change.
                </p>
              ) : null}
            </form>
          )}
        </aside>
      </div>
    </section>
  );
}
