/**
 * Trinity cognitive state store.
 *
 * Shared cache for the footer status control and the Trinity scene. `refresh`
 * pulls the PSI cognitive state and daemon health from the desktop host;
 * consumers subscribe via selectors so the two surfaces never double-fetch.
 */

import { create } from 'zustand';
import { trinityAPI } from '@/infrastructure/api';

export type TrinityDaemonStatus = 'unknown' | 'online' | 'offline';

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

interface TrinityState {
  cognitiveState: TrinityCognitiveState | null;
  status: TrinityDaemonStatus;
  awakened: boolean;
  loading: boolean;
  lastUpdatedAt: number | null;
  refresh: () => Promise<void>;
  setCognitiveState: (state: TrinityCognitiveState | null) => void;
  setStatus: (status: TrinityDaemonStatus) => void;
  setAwakened: (awakened: boolean) => void;
}

export const useTrinityStore = create<TrinityState>((set) => ({
  cognitiveState: null,
  status: 'unknown',
  awakened: false,
  loading: false,
  lastUpdatedAt: null,

  refresh: async () => {
    set({ loading: true });
    try {
      const [state, status] = await Promise.all([
        trinityAPI.getCognitiveState(),
        trinityAPI.getStatus(),
      ]);
      set({
        cognitiveState: state ?? null,
        status: status?.engine_running === false ? 'offline' : 'online',
        awakened: status?.awakened ?? false,
        lastUpdatedAt: Date.now(),
        loading: false,
      });
    } catch {
      set({ status: 'offline', loading: false });
    }
  },

  setCognitiveState: (cognitiveState) => set({ cognitiveState }),
  setStatus: (status) => set({ status }),
  setAwakened: (awakened) => set({ awakened }),
}));