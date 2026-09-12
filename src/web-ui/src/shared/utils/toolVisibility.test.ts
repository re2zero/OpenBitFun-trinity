import { describe, expect, it } from 'vitest';
import {
  COGNITIVE_FRAMEWORK_TOOL_ID,
  isCognitiveFrameworkToolName,
  isUserSelectableToolName,
} from './toolVisibility';

describe('isUserSelectableToolName', () => {
  it.each(['GetToolSpec', 'CallDeferredTool'])(
    'hides the internal gateway tool %s',
    (toolName) => {
      expect(isUserSelectableToolName(toolName)).toBe(false);
    },
  );

  it.each([
    'trinity_cognitive_state',
    'trinity_express',
    'trinity_recall',
    'trinity_memorize',
    'trinity_apply_feedback',
  ])('hides the cognitive framework tool %s', (toolName) => {
    expect(isUserSelectableToolName(toolName)).toBe(false);
    expect(isCognitiveFrameworkToolName(toolName)).toBe(true);
  });

  it('keeps regular tools selectable', () => {
    expect(isUserSelectableToolName('Read')).toBe(true);
    expect(isCognitiveFrameworkToolName('Read')).toBe(false);
  });

  it('manages the framework through one group id', () => {
    expect(isUserSelectableToolName(COGNITIVE_FRAMEWORK_TOOL_ID)).toBe(true);
  });
});
