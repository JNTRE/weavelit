import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { ApplicationShell } from './application-shell';

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json; charset=utf-8' },
  });
}

function statusRegion(): HTMLElement {
  return screen.getByRole('status');
}

describe('ApplicationShell', () => {
  it('renders the loading state while the status request is pending', () => {
    vi.spyOn(globalThis, 'fetch').mockReturnValue(new Promise<Response>(() => {}));

    render(<ApplicationShell />);

    expect(statusRegion().dataset['statusState']).toBe('loading');
    expect(statusRegion().textContent).toBe('Checking the deployment status.');
  });

  it('renders the selected state when an Application Database is selected', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({ lifecycle: 'uninitialized', database_selected: true }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset['statusState']).toBe('available');
    });
    expect(statusRegion().textContent).toBe(
      'An Application Database is selected for this deployment.',
    );
  });

  it('renders the unselected state when no Application Database is selected', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({ lifecycle: 'uninitialized', database_selected: false }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset['statusState']).toBe('available');
    });
    expect(statusRegion().textContent).toBe(
      'No Application Database is selected for this deployment.',
    );
  });

  it('renders the unavailable state when the status request fails', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('ECONNREFUSED 127.0.0.1:8443'));

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset['statusState']).toBe('unavailable');
    });
    expect(statusRegion().textContent).toBe('The deployment status is unavailable.');
  });

  it('renders the unavailable state without leaking a malformed payload', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({ lifecycle: 'uninitialized', database_selected: 'yes', detail: 'secret' }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset['statusState']).toBe('unavailable');
    });
    expect(statusRegion().textContent).toBe('The deployment status is unavailable.');
    expect(document.body.textContent).not.toContain('secret');
    expect(document.body.textContent).not.toContain('yes');
  });

  it('does not render a selection control in the pre-operational shell', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({ lifecycle: 'uninitialized', database_selected: false }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset['statusState']).toBe('available');
    });
    expect(screen.queryAllByRole('button')).toHaveLength(0);
    expect(screen.queryAllByRole('form')).toHaveLength(0);
  });
});
