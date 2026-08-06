import { useCallback, useEffect, useRef, useState } from 'react';

import { fetchPreOperationalStatus, type PreOperationalStatus } from './status-client';

/** Presentation state of the pre-operational status request. */
export type StatusViewState =
  | { readonly kind: 'loading' }
  | { readonly kind: 'available'; readonly status: PreOperationalStatus }
  | { readonly kind: 'unavailable' };

/**
 * Tracks the current status projection and exposes an explicit reload.
 *
 * Status is treated as mutable: `reload` re-requests it, `applyStatus` adopts a
 * projection a state-changing response already returned, and a superseded
 * in-flight request never overwrites a newer result.
 */
export function useDeploymentStatus(): {
  state: StatusViewState;
  reload: () => void;
  applyStatus: (status: PreOperationalStatus) => void;
} {
  const [state, setState] = useState<StatusViewState>({ kind: 'loading' });
  const latestRequest = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const reload = useCallback(() => {
    latestRequest.current += 1;
    const request = latestRequest.current;
    setState({ kind: 'loading' });
    const apply = (next: StatusViewState): void => {
      if (mounted.current && request === latestRequest.current) {
        setState(next);
      }
    };
    void fetchPreOperationalStatus().then(
      (status) => {
        apply({ kind: 'available', status });
      },
      () => {
        apply({ kind: 'unavailable' });
      },
    );
  }, []);

  const applyStatus = useCallback((status: PreOperationalStatus): void => {
    // Supersede any in-flight status request: this projection is newer.
    latestRequest.current += 1;
    if (mounted.current) {
      setState({ kind: 'available', status });
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  return { state, reload, applyStatus };
}
