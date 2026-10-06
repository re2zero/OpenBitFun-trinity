import { describe, expect, it } from 'vitest';
import { exportFileName, parseBeingIdMismatch } from './memoryTransfer';

describe('parseBeingIdMismatch', () => {
  it('extracts both ids from the daemon guard error', () => {
    const mismatch = parseBeingIdMismatch(new Error(
      "being_id mismatch: container 'being-alpha' vs local 'being-beta' — retry with confirm_being_id=true to force",
    ));
    expect(mismatch).toEqual({ containerId: 'being-alpha', localId: 'being-beta' });
  });

  it('passes other errors through as null', () => {
    expect(parseBeingIdMismatch(new Error('path is required'))).toBeNull();
    expect(parseBeingIdMismatch('mindgraph not available')).toBeNull();
  });
});

describe('exportFileName', () => {
  it('formats the local date into the trinity-graph name', () => {
    expect(exportFileName(new Date(2026, 9, 6))).toBe('trinity-graph-2026-10-06.json');
    expect(exportFileName(new Date(2026, 0, 3))).toBe('trinity-graph-2026-01-03.json');
  });
});
