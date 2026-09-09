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

export const FOCUSES = ['respond', 'reflect', 'explore', 'plan', 'idle'] as const;

/** Raw focus string → i18n key suffix under `trinity.focus.*`. */
export function focusLabelKey(focus: string | undefined | null): string {
  return FOCUSES.includes(focus as (typeof FOCUSES)[number]) ? (focus as string) : 'idle';
}

export const NEED_KEYS = ['competence', 'autonomy', 'relatedness', 'certainty', 'growth'] as const;

export type NeedKey = (typeof NEED_KEYS)[number];

/** Needs at or below this level are highlighted as unmet in the UI. */
export const NEED_ATTENTION_THRESHOLD = 0.4;

export interface TrinityMemoryEntry {
  id?: string;
  content?: string;
  kind?: string;
  project?: string;
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
