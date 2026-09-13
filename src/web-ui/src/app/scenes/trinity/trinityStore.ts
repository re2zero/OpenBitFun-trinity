/**
 * Trinity cognitive state store — single source of truth.
 *
 * Dimensions (orthogonal):
 * - conn:      daemon reachability (transient; any RPC failure → disconnected)
 * - awakened:  one-shot ceremony result persisted in trinity.toml
 * - psi:       live cognitive stream; null whenever disconnected (never stale)
 * - cloud:     independent sub-machine (unavailable / unregistered / key
 *              pending / ready); only meaningful while awake
 *
 * Derived phase (see the Trinity repo cognitive UI design doc):
 *   offline = disconnected · dormant = connected && !awakened · awake = otherwise
 *
 * Polling is declared per surface via `useTrinityAutoRefresh(intervalMs)`;
 * pass null to only refresh once on mount.
 */

import { useEffect } from 'react';
import { create } from 'zustand';
import { trinityAPI } from '@/infrastructure/api';

export type TrinityConn = 'disconnected' | 'connected';

/** UI lifecycle phase derived from conn + awakened. */
export type TrinityPhase = 'offline' | 'dormant' | 'awake';

export interface TrinityCognitiveState {
  emotion?: {
    valence?: string;
    valence_f64?: number;
    arousal?: number;
    dominance?: number;
  };
  focus?: string;
  confidence?: number;
  needs?: {
    autonomy?: number;
    certainty?: number;
    competence?: number;
    relatedness?: number;
    growth?: number;
  };
  memory?: {
    total_nodes?: number;
    total_triples?: number;
    vocabulary_size?: number;
    [key: string]: unknown;
  };
  cycle_count?: number;
  [key: string]: unknown;
}

/** Identity + awakening profile from the daemon `get_status` payload. */
export interface TrinityIdentity {
  name?: string;
  persona?: string;
  user_name?: string;
  /** Birthday — generated daemon-side at the awakening moment; absent = not awakened. */
  birthday?: string;
}

/** Raw `cloud.status` payload. */
export interface TrinityCloudStatus {
  enabled?: boolean;
  registered?: boolean;
  user_id?: string;
  engine_running?: boolean;
  relay_url?: string;
  mode?: string;
  pending_ops?: number;
  key_ready?: boolean;
  memory_nodes?: number;
  [key: string]: unknown;
}

interface TrinityState {
  conn: TrinityConn;
  awakened: boolean;
  /** Set right after a successful ceremony so the phase flips to awake
   *  immediately even when the daemon binary predates the `awakened`
   *  status field (stale bundled trinityd). Memory-only; a refresh against
   *  a current daemon overwrites with truth. */
  awakenedLocally: boolean;
  identity: TrinityIdentity | null;
  psi: TrinityCognitiveState | null;
  cloudStatus: TrinityCloudStatus | null;
  lastUpdatedAt: number | null;
  refresh: () => Promise<void>;
  loadCloud: () => Promise<void>;
  markAwakenedLocally: () => void;
}

export const useTrinityStore = create<TrinityState>((set) => ({
  conn: 'disconnected',
  awakened: false,
  awakenedLocally: false,
  identity: null,
  psi: null,
  cloudStatus: null,
  lastUpdatedAt: null,

  refresh: async () => {
    try {
      const [psi, status] = await Promise.all([
        trinityAPI.getCognitiveState(),
        trinityAPI.getStatus(),
      ]);
      // A successful get_status RPC is the definition of "connected".
      const connected = status != null;
      set({
        conn: connected ? 'connected' : 'disconnected',
        awakened: status?.awakened ?? false,
        identity: connected
          ? {
              name: typeof status.name === 'string' ? status.name : undefined,
              persona: typeof status.persona === 'string' ? status.persona : undefined,
              user_name: typeof status.user_name === 'string' ? status.user_name : undefined,
              birthday: typeof status.birthday === 'string' ? status.birthday : undefined,
            }
          : null,
        psi: connected ? (psi ?? null) : null,
        lastUpdatedAt: connected ? Date.now() : null,
      });
    } catch {
      set({ conn: 'disconnected', awakened: false, identity: null, psi: null, lastUpdatedAt: null });
    }
  },

  loadCloud: async () => {
    try {
      set({ cloudStatus: (await trinityAPI.cloudStatus()) ?? null });
    } catch {
      set({ cloudStatus: null });
    }
  },

  markAwakenedLocally: () => set({ awakenedLocally: true, awakened: true }),
}));

/** Derived lifecycle phase — the single fact UI surfaces branch on. */
export function useTrinityPhase(): TrinityPhase {
  const conn = useTrinityStore(s => s.conn);
  const awakened = useTrinityStore(s => s.awakened);
  const awakenedLocally = useTrinityStore(s => s.awakenedLocally);
  if (conn === 'disconnected') return 'offline';
  return awakened || awakenedLocally ? 'awake' : 'dormant';
}

/**
 * Declarative polling: refreshes once on mount, then every `intervalMs`.
 * Pass null for mount-only. Unmount (or interval change) tears down the timer.
 */
export function useTrinityAutoRefresh(intervalMs: number | null): void {
  const refresh = useTrinityStore(s => s.refresh);
  useEffect(() => {
    void refresh();
    if (intervalMs == null) return undefined;
    const timer = window.setInterval(() => { void refresh(); }, intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs, refresh]);
}
