import { describe, expect, it } from 'vitest';
import { buildCognitiveIdentityFiles } from './cognitiveIdentityTemplate';

describe('buildCognitiveIdentityFiles', () => {
  it('returns the three persona files', () => {
    const files = buildCognitiveIdentityFiles({ name: '银月', userName: '公子' });
    expect(Object.keys(files).sort()).toEqual(['IDENTITY.md', 'SOUL.md', 'USER.md']);
  });

  it('carries the awakened name and user into IDENTITY.md', () => {
    const files = buildCognitiveIdentityFiles({ name: '银月', userName: '公子' });
    expect(files['IDENTITY.md']).toContain('name: 银月');
    expect(files['IDENTITY.md']).toContain('公子');
  });

  it('records the persona only when provided', () => {
    const withPersona = buildCognitiveIdentityFiles({ name: '银月', userName: '公子', persona: 'sage' });
    expect(withPersona['IDENTITY.md']).toContain('Persona: sage');

    const withoutPersona = buildCognitiveIdentityFiles({ name: '银月', userName: '公子' });
    expect(withoutPersona['IDENTITY.md']).not.toContain('Persona:');
  });

  it('keeps the bond to the user in USER.md', () => {
    const files = buildCognitiveIdentityFiles({ name: '银月', userName: '公子' });
    expect(files['USER.md']).toContain('**Name:** 公子');
  });
});
