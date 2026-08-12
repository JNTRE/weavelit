import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { InitWorkflow } from "./weavelit-init-workflow";

const RECOVERY_KEY = "AGE-SECRET-KEY-1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SWRYDWG";
const DELIVERY_NONCE = "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8";
const EXPECTED_PROOF = "YiFd573c6n4sQEf_a7lPjRgmL8iz82SBNLt9RBWP-E0";
const CORRELATION = "0123456789abcdef0123456789abcdef";
const PASSWORD = "correct horse battery staple";

const KEY_ONCE_WARNING =
  "This is the only time this recovery key is ever shown. Weavelit cannot display it again, cannot recover it, and cannot issue a replacement. Save it outside Weavelit now, before you continue.";
const CORRECTABLE_AFTER_DELIVERY_MESSAGE =
  "Weavelit did not accept this attempt. The recovery key you saved is still the key this deployment expects, so you do not need a new one and none will be issued. Correct the details below and try again with the same key.";
const CORRECTABLE_BEFORE_DELIVERY_MESSAGE =
  "Weavelit did not accept these details and no recovery key was issued. Correct the details below and try again.";
const CLOSED_MESSAGE =
  "Setup stopped and this Server will not resume it. No further attempt against this Server can succeed, this deployment was not initialized, and the recovery key shown earlier is not usable. Stop this Server, discard its state, and deploy a new Weavelit Server to start again.";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function deliveryResponse(): Response {
  return jsonResponse(
    {
      result: { recovery_key: RECOVERY_KEY, delivery_nonce: DELIVERY_NONCE },
      correlation_id: CORRELATION,
    },
    200,
  );
}

function completionResponse(): Response {
  return jsonResponse({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }, 200);
}

/** Routes the preparation request and the finalization request independently. */
function mockRoutedFetch(routes: {
  prepare: () => Promise<Response>;
  finalize: () => Promise<Response>;
}) {
  return vi
    .spyOn(globalThis, "fetch")
    .mockImplementation((input: unknown) =>
      input === "/api/v1/init/recovery-key" ? routes.prepare() : routes.finalize(),
    );
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
  });

  it("fails closed when the proof cannot be derived from the delivered key", async () => {
    mockRoutedFetch({
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
});
