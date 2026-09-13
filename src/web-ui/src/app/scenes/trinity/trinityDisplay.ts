/**
 * Shared display mappings for Trinity cognitive state.
 *
 * Translate raw daemon enums (emotion valence, focus, need keys) into
 * i18n keys and data attributes consumed by the sidebar being-section,
 * the footer status panel, and the full scene.
 */

export const EMOTION_VALENCES = [
  'positive_high',
  'positive_mild',
  'neutral',
  'curious',
  'confused',
  'negative_mild',
  'negative_high',
] as const;

export type EmotionValence = (typeof EMOTION_VALENCES)[number];

/** Raw valence string → i18n key suffix under `trinity.emotion.*`. */
export function emotionLabelKey(valence: string | undefined | null): string {
  return EMOTION_VALENCES.includes(valence as EmotionValence) ? (valence as string) : 'neutral';
}

/**
 * Numeric position of a valence on the engine's positive→negative axis.
 *
 * Mirrors `EmotionalValence::as_f64` in the cognitive engine: the cognition
 * history payload carries the valence as a label only (no numeric counterpart),
 * so the trend sparkline plots that axis score. Unknown labels return null and
 * are dropped from the series.
 */
export function valenceScore(valence: string | undefined | null): number | null {
  switch (valence) {
    case 'positive_high':
      return 1;
    case 'positive_mild':
      return 0.5;
    case 'neutral':
      return 0;
    case 'negative_mild':
      return -0.5;
    case 'negative_high':
      return -1;
    case 'curious':
      return 0.3;
    case 'confused':
      return -0.2;
    default:
      return null;
  }
}

export const FOCUSES = ['respond', 'reflect', 'explore', 'plan', 'idle'] as const;

/** Raw focus string → i18n key suffix under `trinity.focus.*`. */
export function focusLabelKey(focus: string | undefined | null): string {
  return FOCUSES.includes(focus as (typeof FOCUSES)[number]) ? (focus as string) : 'idle';
}

/**
 * Localized short persona name for badges. Known presets map to
 * `trinity.personaShort.*`; unknown values fall back to the raw string.
 */
export function personaLabel(persona: string, t: (key: string) => string): string {
  const key = `trinity.personaShort.${persona}`;
  const label = t(key);
  return label === key ? persona : label;
}

export const NEED_KEYS = ['competence', 'autonomy', 'relatedness', 'certainty', 'growth'] as const;

export type NeedKey = (typeof NEED_KEYS)[number];

/** Needs at or below this level are highlighted as unmet in the UI. */
export const NEED_ATTENTION_THRESHOLD = 0.4;

/** Needs at or above this level read as satisfied. */
export const NEED_SATISFIED_THRESHOLD = 0.8;

/**
 * Visual state of one need bar. `low` asks for attention, `high` marks a
 * satisfied drive, `normal` is the healthy middle band; each state has its own
 * color in the need-bar styles shared by the sidebar panel and the full scene.
 */
export type NeedState = 'low' | 'normal' | 'high';

export function needState(value: number): NeedState {
  if (value <= NEED_ATTENTION_THRESHOLD) return 'low';
  if (value >= NEED_SATISFIED_THRESHOLD) return 'high';
  return 'normal';
}

/** Canonical percentage rendering — always one decimal place (99.4%). */
export function formatPercent(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

export interface TrinityMemoryEntry {
  id?: string;
  content?: string;
  kind?: string;
  layer?: string;
  strength?: number;
  /** Unix timestamp in seconds (daemon MindGraph convention). */
  timestamp?: number;
  created_at?: string;
  ts?: string;
  [key: string]: unknown;
}

/** Normalize a memory-timeline payload into a flat entry list. */
export function normalizeMemoryItems(payload: unknown): TrinityMemoryEntry[] {
  const items = (payload as { items?: unknown })?.items;
  return Array.isArray(items) ? (items as TrinityMemoryEntry[]) : [];
}

/** Best-effort timestamp (ms) for a memory entry. */
export function memoryEntryTime(entry: TrinityMemoryEntry): number | null {
  if (typeof entry.timestamp === 'number' && Number.isFinite(entry.timestamp)) {
    // Daemon sends seconds; guard against ms-level values anyway.
    return entry.timestamp > 1e12 ? entry.timestamp : entry.timestamp * 1000;
  }
  const raw = entry.created_at ?? entry.ts;
  if (typeof raw !== 'string' && typeof raw !== 'number') return null;
  const value = typeof raw === 'number' ? raw : Date.parse(raw);
  return Number.isFinite(value) ? value : null;
}

/** Compact `MM-DD HH:mm` rendering for memory entries; empty when unknown. */
export function formatMemoryTime(entry: TrinityMemoryEntry): string {
  const ms = memoryEntryTime(entry);
  if (ms == null) return '';
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
