import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { InitWorkflow, settlePreparationRejection } from "./weavelit-init-workflow";

const RECOVERY_KEY = "AGE-SECRET-KEY-1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SWRYDWG";
const DELIVERY_NONCE = "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8";
const EXPECTED_PROOF = "YiFd573c6n4sQEf_a7lPjRgmL8iz82SBNLt9RBWP-E0";
const CORRELATION = "0123456789abcdef0123456789abcdef";
const PASSWORD = "correct horse battery staple";
const RECONCILIATION_CAPABILITY = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi";

const KEY_ONCE_WARNING =
  "This is the only time this recovery key is ever shown. Weavelit cannot display it again, cannot recover it, and cannot issue a replacement. Save it outside Weavelit now, before you continue.";
const CORRECTABLE_AFTER_DELIVERY_MESSAGE =
  "Weavelit did not accept this attempt. The recovery key you saved is still the key this deployment expects, so you do not need a new one and none will be issued. Correct the details below and try again with the same key.";
const CORRECTABLE_BEFORE_DELIVERY_MESSAGE =
  "Weavelit did not accept these details and no recovery key was issued. Correct the details below and try again.";
const CLOSED_MESSAGE =
  "Setup stopped and this Server will not resume it. No further attempt against this Server can succeed, this deployment was not initialized, and the recovery key shown earlier is not usable. Stop this Server, discard its state, and deploy a new Weavelit Server to start again.";
const INDETERMINATE_MESSAGE =
  "This attempt reported no outcome, so whether this deployment was initialized is not yet known. Keep the recovery key you saved: it is still the only key this deployment can be restored with, and none will be issued again. Check again to see whether setup completed, or try again with the same key.";
const INDETERMINATE_CHECKING_MESSAGE = "Checking whether setup completed.";

const PREPARE_PATH = "/api/v1/init/recovery-key";
const FINALIZE_PATH = "/api/v1/init";
const RECONCILIATION_PATH = "/api/v1/lifecycle/reconciliation";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function deliveryResponse(): Response {
  return jsonResponse(
    {
      result: {
        recovery_key: RECOVERY_KEY,
        delivery_nonce: DELIVERY_NONCE,
        reconciliation_capability: RECONCILIATION_CAPABILITY,
      },
      correlation_id: CORRELATION,
    },
    200,
  );
}

function completionResponse(): Response {
  return jsonResponse({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }, 200);
}

/** The listener's answer when it stopped waiting before the route reported. */
function timeoutResponse(): Response {
  return jsonResponse({ error: "gateway_timeout", correlation_id: CORRELATION }, 504);
}

/** An accepted finalization whose completion body never arrived intact. */
function truncatedCompletionResponse(): Response {
  return new Response('{"result":{"lifecycle":"ini', {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

/** The exact confirmation a matching submitted capability receives. */
function confirmedReconciliation(): Promise<Response> {
  return Promise.resolve(jsonResponse({ result: "reconciliation_confirmed" }, 200));
}

/** The fixed answer a non-matching submitted capability receives. */
function notFoundReconciliation(): Promise<Response> {
  return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
}

/**
 * Routes the preparation, finalization, and reconciliation requests apart.
 *
 * A reconciliation a test did not stage is refused rather than answered, so a
 * test that never expected one cannot silently read one.
 */
function mockRoutedFetch(routes: {
  prepare: () => Promise<Response>;
  finalize: () => Promise<Response>;
  reconciliation?: () => Promise<Response>;
}) {
  return vi.spyOn(globalThis, "fetch").mockImplementation((input: unknown) => {
    if (input === PREPARE_PATH) {
      return routes.prepare();
    }
    if (input === RECONCILIATION_PATH) {
      return routes.reconciliation === undefined
        ? Promise.reject(new Error("unrouted reconciliation"))
        : routes.reconciliation();
    }
    return routes.finalize();
  });
}

/** Counts the calls a mocked fetch received for one exact request target. */
function callsTo(mock: { mock: { calls: unknown[][] } }, path: string): number {
  return mock.mock.calls.filter((call) => call[0] === path).length;
}

function section(): HTMLElement {
  return document.querySelector<HTMLElement>("section.shell__init")!;
}

function step(): string | undefined {
  return document.querySelector<HTMLElement>("[data-init-step]")?.dataset.initStep;
}

function keyInput(): HTMLInputElement {
  return screen.getByLabelText("Recovery key for this deployment");
}

function acknowledgement(): HTMLInputElement {
  return screen.getByLabelText("I have saved this recovery key.");
}

function button(name: string): HTMLButtonElement {
  return screen.getByRole("button", { name });
}

function enterDetails(overrides: { username?: string; displayName?: string } = {}): void {
  fireEvent.change(screen.getByLabelText("System Log module"), { target: { value: "sqlite" } });
  fireEvent.change(screen.getByLabelText("Audit Log module"), { target: { value: "sqlite" } });
  fireEvent.change(screen.getByLabelText("Username"), {
    target: { value: overrides.username ?? "administrator" },
  });
  fireEvent.change(screen.getByLabelText("Display name (optional)"), {
    target: { value: overrides.displayName ?? "First Administrator" },
  });
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: PASSWORD } });
}

async function reachKeyStep(): Promise<void> {
  enterDetails();
  fireEvent.click(button("Prepare recovery key"));
  await waitFor(() => {
    expect(step()).toBe("key");
  });
}

async function reachReviewStep(): Promise<void> {
  await reachKeyStep();
  fireEvent.click(acknowledgement());
  fireEvent.click(button("Continue to review"));
  await waitFor(() => {
    expect(step()).toBe("review");
  });
}

function stubClipboard(writeText: () => Promise<void>): void {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
}

afterEach(() => {
  Reflect.deleteProperty(navigator, "clipboard");
  window.localStorage.clear();
  window.sessionStorage.clear();
});

/**
 * Returns the exact request body text a call sent.
 *
 * Asserted to be a string rather than coerced, because a non-string body would
 * stringify to something like `[object Blob]` and make a "the key never
 * reached the wire" assertion pass without inspecting anything.
 */
function bodyText(mock: { mock: { calls: unknown[][] } }, call: number): string {
  const body = (mock.mock.calls[call]?.[1] as RequestInit | undefined)?.body;
  expect(typeof body).toBe("string");
  return body as string;
}

describe("InitWorkflow", () => {
  it("presents both log assignments, the administrator fields, and a disabled action", () => {
    render(<InitWorkflow onCompleted={() => {}} />);

    expect(section().dataset.initState).toBe("details");
    expect(step()).toBe("details");
    expect(screen.getByLabelText("System Log module")).toBeDefined();
    expect(screen.getByLabelText("Audit Log module")).toBeDefined();
    expect(screen.getByLabelText<HTMLInputElement>("Password").type).toBe("password");
    expect(button("Prepare recovery key").disabled).toBe(true);
  });

  it("enables preparation only once both assignments and the identity are supplied", () => {
    render(<InitWorkflow onCompleted={() => {}} />);

    fireEvent.change(screen.getByLabelText("System Log module"), { target: { value: "sqlite" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "administrator" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: PASSWORD } });
    expect(button("Prepare recovery key").disabled).toBe(true);

    fireEvent.change(screen.getByLabelText("Audit Log module"), { target: { value: "sqlite" } });
    expect(button("Prepare recovery key").disabled).toBe(false);
  });

  it("submits the assignments and identity to the preparation route", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachKeyStep();

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const body = JSON.parse(bodyText(fetchMock, 0)) as Record<string, unknown>;
    expect(body.system_log).toBe("system-log");
    expect(body.audit_log).toBe("audit-log");
    expect(body.log_modules).toHaveLength(2);
  });

  it("displays the delivered key once, with an explicit statement that it is never shown again", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachKeyStep();

    expect(keyInput().value).toBe(RECOVERY_KEY);
    expect(keyInput().readOnly).toBe(true);
    expect(screen.getByText(KEY_ONCE_WARNING).getAttribute("role")).toBe("alert");
    expect(button("Copy recovery key")).toBeDefined();
  });

  it("gates continuing on an explicit acknowledgement that the key was saved", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachKeyStep();

    expect(acknowledgement().checked).toBe(false);
    expect(button("Continue to review").disabled).toBe(true);

    fireEvent.click(acknowledgement());
    expect(button("Continue to review").disabled).toBe(false);

    fireEvent.click(acknowledgement());
    expect(button("Continue to review").disabled).toBe(true);
  });

  it("copies the delivered key on request", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    stubClipboard(writeText);
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachKeyStep();
    fireEvent.click(button("Copy recovery key"));

    await screen.findByText("The recovery key was copied to the clipboard.");
    expect(writeText).toHaveBeenCalledWith(RECOVERY_KEY);
  });

  it("reports an unavailable clipboard rather than silently failing to copy", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachKeyStep();
    fireEvent.click(button("Copy recovery key"));

    await screen.findByText(
      "The recovery key could not be copied. Select it and copy it manually.",
    );
  });

  it("reviews the assignments and identity without repeating the password or the key", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachReviewStep();

    expect(document.querySelector("[data-init-review='username']")?.textContent).toBe(
      "administrator",
    );
    expect(document.querySelector("[data-init-review='display-name']")?.textContent).toBe(
      "First Administrator",
    );
    expect(document.querySelector("[data-init-review='system-log']")?.textContent).toBe("SQLite");
    expect(document.querySelector("[data-init-review='audit-log']")?.textContent).toBe("SQLite");
    expect(document.body.textContent).not.toContain(PASSWORD);
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
  });

  it("reports an unset display name rather than an empty review value", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    enterDetails({ displayName: "" });
    fireEvent.click(button("Prepare recovery key"));
    await waitFor(() => {
      expect(step()).toBe("key");
    });
    fireEvent.click(acknowledgement());
    fireEvent.click(button("Continue to review"));

    await waitFor(() => {
      expect(document.querySelector("[data-init-review='display-name']")?.textContent).toBe(
        "Not set",
      );
    });
  });

  it("finalizes with a proof derived over the delivery nonce and reports completion", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const body = JSON.parse(bodyText(fetchMock, 1)) as Record<string, unknown>;
    expect(body.recovery_key_proof).toBe(EXPECTED_PROOF);
    // Only the proof is submitted: the key itself never returns to the Server.
    expect(bodyText(fetchMock, 1)).not.toContain(RECOVERY_KEY);
  });

  it("lets an actionable failure be corrected and retried with the existing key", async () => {
    let finalizations = 0;
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => {
        finalizations += 1;
        return Promise.resolve(
          finalizations === 1
            ? jsonResponse({ error: "recovery_key_confirmation_invalid" }, 400)
            : completionResponse(),
        );
      },
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(CORRECTABLE_AFTER_DELIVERY_MESSAGE);
    expect(section().dataset.initState).toBe("details");
    // The key is neither redisplayed nor reissued, and no second preparation is
    // offered, because the deployment still expects the key already saved.
    expect(screen.queryByText(KEY_ONCE_WARNING)).toBeNull();
    expect(screen.queryByRole("button", { name: "Prepare recovery key" })).toBeNull();
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);

    // The password did not outlive the attempt it drove, so it must be entered again.
    expect(screen.getByLabelText<HTMLInputElement>("Password").value).toBe("");
    expect(button("Return to review").disabled).toBe(true);

    fireEvent.change(screen.getByLabelText("Password"), { target: { value: PASSWORD } });
    fireEvent.click(button("Return to review"));
    fireEvent.click(button("Complete setup"));

    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    // The retry reused the delivered key, so no further preparation was issued.
    expect(
      fetchMock.mock.calls.filter((call) => call[0] === "/api/v1/init/recovery-key"),
    ).toHaveLength(1);
  });

  it("presents a permanent failure honestly and withdraws every retry control", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(jsonResponse({ error: "not_found" }, 404)),
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(CLOSED_MESSAGE);
    expect(section().dataset.initState).toBe("closed");
    expect(document.querySelector("[data-init-error]")?.getAttribute("data-init-error")).toBe(
      "not_found",
    );
    expect(screen.queryAllByRole("button")).toHaveLength(0);
    expect(completed).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
    expect(document.body.textContent).not.toContain(PASSWORD);
  });

  it("treats an ambiguous finalization failure as correctable so the key is not abandoned", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(jsonResponse({ error: "initialization_failed" }, 500)),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(CORRECTABLE_AFTER_DELIVERY_MESSAGE);
    expect(section().dataset.initState).toBe("details");
  });

  it("completes a finalization that reported nothing once reconciliation confirms it", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(timeoutResponse()),
      reconciliation: confirmedReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    // Only the matching submitted capability confirms this deployment was
    // sealed with the delivered key and the workflow is done with that key.
    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(1);
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
    expect(document.body.textContent).not.toContain(PASSWORD);
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("keeps the delivered key when nothing can yet settle a finalization that reported nothing", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(timeoutResponse()),
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(INDETERMINATE_MESSAGE);
    expect(section().dataset.initState).toBe("indeterminate");
    expect(document.querySelector("[data-init-error]")?.getAttribute("data-init-error")).toBe(
      "gateway_timeout",
    );
    expect(completed).not.toHaveBeenCalled();
    expect(button("Check again")).toBeDefined();
    expect(button("Try setup again")).toBeDefined();
    expect(document.body.textContent).not.toContain(PASSWORD);

    // The delivered key survived the outcome that reported nothing: returning
    // to the details step offers the retry path rather than a second
    // preparation, and no further key was ever requested.
    fireEvent.click(button("Try setup again"));
    expect(button("Return to review")).toBeDefined();
    expect(screen.queryByRole("button", { name: "Prepare recovery key" })).toBeNull();
    expect(callsTo(fetchMock, PREPARE_PATH)).toBe(1);
  });

  it("completes a finalization whose transport failed once reconciliation confirms it", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.reject(new Error("ECONNRESET")),
      reconciliation: confirmedReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    // The request may still have reached the Server and sealed the deployment,
    // so the lost answer is settled against its submitted capability rather
    // than presented as a failure that never happened.
    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(1);
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
    expect(document.body.textContent).not.toContain(PASSWORD);
  });

  it("keeps the delivered key when a finalization's completion body never arrived", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(truncatedCompletionResponse()),
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(INDETERMINATE_MESSAGE);
    expect(section().dataset.initState).toBe("indeterminate");
    expect(document.querySelector("[data-init-error]")?.getAttribute("data-init-error")).toBe(
      "init_failed",
    );
    expect(completed).not.toHaveBeenCalled();

    // The key survived a body that reported nothing: the retry path is the
    // existing key's, and no second preparation was ever requested.
    fireEvent.click(button("Try setup again"));
    expect(button("Return to review")).toBeDefined();
    expect(screen.queryByRole("button", { name: "Prepare recovery key" })).toBeNull();
    expect(callsTo(fetchMock, PREPARE_PATH)).toBe(1);
  });

  it("rechecks on request and completes only once reconciliation confirms it", async () => {
    let reconciliations = 0;
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(timeoutResponse()),
      reconciliation: () => {
        reconciliations += 1;
        return reconciliations === 1 ? notFoundReconciliation() : confirmedReconciliation();
      },
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(INDETERMINATE_MESSAGE);
    expect(completed).not.toHaveBeenCalled();

    fireEvent.click(button("Check again"));
    await screen.findByText(INDETERMINATE_CHECKING_MESSAGE);
    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });

    expect(reconciliations).toBe(2);
    // The recheck settled it on its own: no second finalization was submitted.
    expect(callsTo(fetchMock, FINALIZE_PATH)).toBe(1);
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
  });

  it("retries a finalization that reported nothing with its original locked payload", async () => {
    let finalizations = 0;
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => {
        finalizations += 1;
        return Promise.resolve(finalizations === 1 ? timeoutResponse() : completionResponse());
      },
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(INDETERMINATE_MESSAGE);
    fireEvent.click(button("Try setup again"));

    expect(screen.getByLabelText<HTMLInputElement>("System Log module").disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>("Audit Log module").disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>("Username").disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>("Display name (optional)").disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>("Password").disabled).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>("Password").value).toBe(PASSWORD);
    expect(button("Return to review").disabled).toBe(false);
    fireEvent.click(button("Return to review"));
    fireEvent.click(button("Complete setup"));

    await waitFor(() => {
      expect(completed).toHaveBeenCalledTimes(1);
    });
    expect(callsTo(fetchMock, PREPARE_PATH)).toBe(1);
    // The retry proved possession of the very key delivered before the timeout.
    const body = JSON.parse(bodyText(fetchMock, fetchMock.mock.calls.length - 1)) as Record<
      string,
      unknown
    >;
    expect(body.recovery_key_proof).toBe(EXPECTED_PROOF);
  });

  it.each([
    [409, "already_initialized"],
    [404, "not_found"],
  ])(
    "completes when a retry is answered %i by the setup its unsettled attempt committed",
    async (status, code) => {
      let finalizations = 0;
      let probes = 0;
      const fetchMock = mockRoutedFetch({
        prepare: () => Promise.resolve(deliveryResponse()),
        finalize: () => {
          finalizations += 1;
          return Promise.resolve(
            finalizations === 1 ? timeoutResponse() : jsonResponse({ error: code }, status),
          );
        },
        reconciliation: () => {
          probes += 1;
          return probes === 1 ? notFoundReconciliation() : confirmedReconciliation();
        },
      });
      const completed = vi.fn();

      render(<InitWorkflow onCompleted={completed} />);
      await reachReviewStep();
      fireEvent.click(button("Complete setup"));

      await screen.findByText(INDETERMINATE_MESSAGE);
      fireEvent.click(button("Try setup again"));
      fireEvent.change(screen.getByLabelText("Password"), { target: { value: PASSWORD } });
      fireEvent.click(button("Return to review"));
      fireEvent.click(button("Complete setup"));

      // The retry was answered about the original attempt, not about itself,
      // and the surface it was reconciled against proves that original
      // committed. The shell is advanced rather than failed closed.
      await waitFor(() => {
        expect(completed).toHaveBeenCalledTimes(1);
      });
      expect(section().dataset.initState).not.toBe("closed");
      expect(callsTo(fetchMock, PREPARE_PATH)).toBe(1);
      expect(document.body.textContent).not.toContain(RECOVERY_KEY);
      expect(document.body.textContent).not.toContain(PASSWORD);
    },
  );

  it("keeps a retry answered already_initialized unsettled when nothing proves it", async () => {
    let finalizations = 0;
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => {
        finalizations += 1;
        return Promise.resolve(
          finalizations === 1
            ? timeoutResponse()
            : jsonResponse({ error: "already_initialized" }, 409),
        );
      },
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(INDETERMINATE_MESSAGE);
    fireEvent.click(button("Try setup again"));
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: PASSWORD } });
    fireEvent.click(button("Return to review"));
    fireEvent.click(button("Complete setup"));

    await waitFor(() => {
      expect(document.querySelector("[data-init-error]")?.getAttribute("data-init-error")).toBe(
        "already_initialized",
      );
    });
    // The code is reconciled, never believed: a Server that failed closed
    // reports it too, so with no evidence the attempt stays unsettled and the
    // delivered key stays with it.
    expect(section().dataset.initState).toBe("indeterminate");
    expect(completed).not.toHaveBeenCalled();
    fireEvent.click(button("Try setup again"));
    expect(button("Return to review")).toBeDefined();
    expect(callsTo(fetchMock, PREPARE_PATH)).toBe(1);
  });

  it("still fails closed, and drops the key, on a determinate failure after an unsettled attempt", async () => {
    let finalizations = 0;
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => {
        finalizations += 1;
        return Promise.resolve(
          finalizations === 1 ? timeoutResponse() : jsonResponse({ error: "init_refused" }, 409),
        );
      },
      reconciliation: notFoundReconciliation,
    });
    const completed = vi.fn();

    render(<InitWorkflow onCompleted={completed} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));

    await screen.findByText(INDETERMINATE_MESSAGE);
    fireEvent.click(button("Try setup again"));
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: PASSWORD } });
    fireEvent.click(button("Return to review"));
    fireEvent.click(button("Complete setup"));

    // A rejection only the finalization route itself can report is evidence
    // about this attempt, so it settles and the key it held is dropped.
    await screen.findByText(CLOSED_MESSAGE);
    expect(section().dataset.initState).toBe("closed");
    expect(completed).not.toHaveBeenCalled();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
    // Nothing was reconciled: only the first attempt ever reached the surface.
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(1);
  });

  it("lets a rejected preparation be corrected and retried", async () => {
    let preparations = 0;
    mockRoutedFetch({
      prepare: () => {
        preparations += 1;
        return Promise.resolve(
          preparations === 1 ? jsonResponse({ error: "bad_request" }, 400) : deliveryResponse(),
        );
      },
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    enterDetails();
    fireEvent.click(button("Prepare recovery key"));

    await screen.findByText(CORRECTABLE_BEFORE_DELIVERY_MESSAGE);
    expect(section().dataset.initState).toBe("details");

    fireEvent.click(button("Prepare recovery key"));
    await waitFor(() => {
      expect(step()).toBe("key");
    });
  });

  it("fails closed when a preparation rejection cannot be corrected", async () => {
    expect(settlePreparationRejection(PASSWORD, "already_initialized")).toEqual({
      password: "",
      state: { kind: "closed", code: "already_initialized" },
    });

    mockRoutedFetch({
      prepare: () => Promise.resolve(jsonResponse({ error: "already_initialized" }, 409)),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    enterDetails();
    fireEvent.click(button("Prepare recovery key"));

    await screen.findByText(CLOSED_MESSAGE);
    expect(document.querySelector("[data-init-error]")?.getAttribute("data-init-error")).toBe(
      "already_initialized",
    );
    expect(section().dataset.initState).toBe("closed");
  });

  it("fails closed when the proof cannot be derived from the delivered key", async () => {
    const fetchMock = mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachReviewStep();
    vi.spyOn(globalThis.crypto.subtle, "sign").mockRejectedValue(new Error("unsupported"));
    fireEvent.click(button("Complete setup"));

    await screen.findByText(CLOSED_MESSAGE);
    expect(document.querySelector("[data-init-error]")?.getAttribute("data-init-error")).toBe(
      "init_failed",
    );
    // This failure carries the same fixed code as one that reported no outcome,
    // but it is determinate: no finalization was ever submitted, so nothing is
    // reconciled and the key it could never be used with is dropped.
    expect(callsTo(fetchMock, FINALIZE_PATH)).toBe(0);
    expect(callsTo(fetchMock, RECONCILIATION_PATH)).toBe(0);
    expect(document.body.textContent).not.toContain(RECOVERY_KEY);
  });

  it("writes neither the password nor the delivered key to browser storage or the URL", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));
    await waitFor(() => {
      expect(globalThis.fetch).toHaveBeenCalledTimes(2);
    });

    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.length).toBe(0);
    expect(document.cookie).toBe("");
    expect(window.location.href).not.toContain(RECOVERY_KEY);
    expect(window.location.href).not.toContain(PASSWORD);
  });

  it("keeps the proof and the log assignments out of storage and the URL too", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));
    await waitFor(() => {
      expect(globalThis.fetch).toHaveBeenCalledTimes(2);
    });

    // The proof is derived material for the key, so it is held to the same rule
    // as the key itself, as is the delivery nonce it is bound to.
    for (const secret of [
      EXPECTED_PROOF,
      DELIVERY_NONCE,
      RECONCILIATION_CAPABILITY,
      RECOVERY_KEY,
      PASSWORD,
    ]) {
      // A needle that never appears anywhere would make every check below pass
      // without proving anything, so its presence is established first.
      expect(secret.length).toBeGreaterThan(0);
      expect(window.location.href).not.toContain(secret);
      expect(document.cookie).not.toContain(secret);
      expect(JSON.stringify(window.localStorage)).not.toContain(secret);
      expect(JSON.stringify(window.sessionStorage)).not.toContain(secret);
    }
  });

  it("writes nothing to the console on any failure path", async () => {
    const written: unknown[] = [];
    const methods = ["log", "info", "warn", "error", "debug", "trace"] as const;
    const spies = methods.map((method) =>
      vi.spyOn(console, method).mockImplementation((...args: unknown[]) => {
        written.push(...args);
      }),
    );
    try {
      mockRoutedFetch({
        prepare: () => Promise.resolve(deliveryResponse()),
        finalize: () => Promise.reject(new Error(`transport carrying ${RECOVERY_KEY}`)),
        reconciliation: notFoundReconciliation,
      });

      render(<InitWorkflow onCompleted={() => {}} />);
      await reachReviewStep();
      fireEvent.click(button("Complete setup"));
      await waitFor(() => {
        expect(section().textContent).toContain(INDETERMINATE_MESSAGE);
      });

      expect(written).toEqual([]);
      // The rejection deliberately carries the key in its message, so this also
      // pins that a thrown error is never surfaced verbatim.
      expect(section().textContent).not.toContain(RECOVERY_KEY);
    } finally {
      for (const spy of spies) {
        spy.mockRestore();
      }
    }
  });

  it("renders no submitted secret in any failure detail", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () =>
        Promise.resolve(
          jsonResponse({ error: "initialization_failed", correlation_id: CORRELATION }, 500),
        ),
    });

    render(<InitWorkflow onCompleted={() => {}} />);
    await reachReviewStep();
    fireEvent.click(button("Complete setup"));
    await waitFor(() => {
      expect(section().textContent).toContain(CORRECTABLE_AFTER_DELIVERY_MESSAGE);
    });

    // The rendered text is read from the failure region rather than the whole
    // workflow, because the key is legitimately still on the page above it.
    const rendered = document.querySelector<HTMLElement>(".shell__init-failure")?.textContent ?? "";
    expect(rendered.length).toBeGreaterThan(0);
    for (const secret of [PASSWORD, EXPECTED_PROOF, DELIVERY_NONCE]) {
      expect(rendered).not.toContain(secret);
    }
  });

  it("offers no resume or reconstruction path after the page is reloaded", async () => {
    mockRoutedFetch({
      prepare: () => Promise.resolve(deliveryResponse()),
      finalize: () => Promise.resolve(completionResponse()),
    });

    const first = render(<InitWorkflow onCompleted={() => {}} />);
    await reachKeyStep();
    expect(keyInput().value).toBe(RECOVERY_KEY);

    // Unmounting and mounting a new workflow is what a reload does to this
    // page: the delivered key lived only in the memory that was just discarded.
    first.unmount();
    render(<InitWorkflow onCompleted={() => {}} />);

    expect(step()).toBe("details");
    expect(screen.queryByLabelText("Recovery key for this deployment")).toBeNull();
    expect(screen.queryByRole("button", { name: "Complete setup" })).toBeNull();
    expect(section().textContent).not.toContain(RECOVERY_KEY);
    // Nothing survived that could reconstruct it, so no resume is even possible.
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.length).toBe(0);
    expect(document.cookie).toBe("");
  });
});
