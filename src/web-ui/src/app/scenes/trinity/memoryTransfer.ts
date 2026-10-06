/**
 * Helpers for the diary's export/import flows (T11, consuming the T10 RPCs).
 *
 * The daemon's memory.import guard rejects a cross-identity import with an
 * error string that carries both being ids; the UI parses it to drive the
 * explicit confirmation step (trinity-graph spec §1 migration gate).
 */

export interface BeingIdMismatch {
  containerId: string;
  localId: string;
}

const MISMATCH_PATTERN = /being_id mismatch: container '([^']*)' vs local '([^']*)'/;

/** Extract both ids from the daemon's being_id-mismatch error, if it is one. */
export function parseBeingIdMismatch(error: unknown): BeingIdMismatch | null {
  const message = error instanceof Error ? error.message : String(error ?? '');
  const match = MISMATCH_PATTERN.exec(message);
  if (!match) return null;
  return { containerId: match[1], localId: match[2] };
}

/** Default export file name: trinity-graph-YYYY-MM-DD.json (local date). */
export function exportFileName(today = new Date()): string {
  const year = today.getFullYear();
  const month = String(today.getMonth() + 1).padStart(2, '0');
  const day = String(today.getDate()).padStart(2, '0');
  return `trinity-graph-${year}-${month}-${day}.json`;
}
