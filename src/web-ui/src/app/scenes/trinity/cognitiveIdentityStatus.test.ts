import { describe, expect, it } from 'vitest';
import {
  COGNITIVE_BEING_ASSISTANT_ID,
  needsCognitiveIdentity,
  type CognitiveIdentityWorkspace,
} from './cognitiveIdentityStatus';

function trinityWorkspace(
  overrides: Partial<CognitiveIdentityWorkspace> = {},
): CognitiveIdentityWorkspace {
  return {
    id: 'ws-trinity',
    assistantId: COGNITIVE_BEING_ASSISTANT_ID,
    identity: { name: '银月' },
    ...overrides,
  };
}

const namedWorkspace: CognitiveIdentityWorkspace = {
  id: 'ws-named',
  assistantId: 'named',
  identity: { name: 'Ada' },
};

describe('needsCognitiveIdentity', () => {
  it('stays hidden while the engine is offline', () => {
    expect(needsCognitiveIdentity('offline', [], null)).toBe(false);
  });

  it('stays hidden while the being is dormant', () => {
    expect(needsCognitiveIdentity('dormant', [namedWorkspace], 'ws-named')).toBe(false);
  });

  it('shows when no Trinity workspace exists', () => {
    expect(needsCognitiveIdentity('awake', [namedWorkspace], 'ws-named')).toBe(true);
  });

  it('shows when another assistant holds the primary role', () => {
    expect(needsCognitiveIdentity('awake', [namedWorkspace, trinityWorkspace()], 'ws-named')).toBe(true);
  });

  it('shows when the Trinity workspace was reset to generic templates', () => {
    expect(needsCognitiveIdentity('awake', [trinityWorkspace({ identity: { name: '' } })], 'ws-trinity')).toBe(true);
    expect(needsCognitiveIdentity('awake', [trinityWorkspace({ identity: null })], 'ws-trinity')).toBe(true);
  });

  it('stays hidden when the Trinity workspace holds a name and the primary role', () => {
    expect(needsCognitiveIdentity('awake', [namedWorkspace, trinityWorkspace()], 'ws-trinity')).toBe(false);
  });
});
