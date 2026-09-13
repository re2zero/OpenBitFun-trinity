import { describe, expect, it } from 'vitest';

import { EMOTION_VALENCES, formatPercent, valenceScore } from './trinityDisplay';

describe('valenceScore', () => {
  it('mirrors the cognitive engine EmotionalValence::as_f64 scale', () => {
    expect(valenceScore('positive_high')).toBe(1);
    expect(valenceScore('positive_mild')).toBe(0.5);
    expect(valenceScore('neutral')).toBe(0);
    expect(valenceScore('curious')).toBe(0.3);
    expect(valenceScore('confused')).toBe(-0.2);
    expect(valenceScore('negative_mild')).toBe(-0.5);
    expect(valenceScore('negative_high')).toBe(-1);
  });

  it('scores every known valence so the trend line always has data', () => {
    for (const valence of EMOTION_VALENCES) {
      expect(valenceScore(valence)).not.toBeNull();
    }
  });

  it('drops unknown or missing labels', () => {
    expect(valenceScore('elated')).toBeNull();
    expect(valenceScore(undefined)).toBeNull();
    expect(valenceScore(null)).toBeNull();
  });
});

describe('formatPercent', () => {
  it('renders one decimal place, including at the bounds', () => {
    expect(formatPercent(0)).toBe('0.0%');
    expect(formatPercent(0.994)).toBe('99.4%');
    expect(formatPercent(1)).toBe('100.0%');
  });
});
