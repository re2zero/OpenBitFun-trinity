const INTERNAL_GATEWAY_TOOL_NAMES: ReadonlySet<string> = new Set([
  'GetToolSpec',
  'CallDeferredTool',
]);

/**
 * Trinity cognitive framework tools: system-embedded capabilities managed as
 * one group by the `trinity_cognitive` toggle. They never take part in the
 * "other tools / builtin tools" selection or counts (see the cognitive
 * framework manifest expansion).
 */
const COGNITIVE_FRAMEWORK_TOOL_NAMES: ReadonlySet<string> = new Set([
  'trinity_cognitive_state',
  'trinity_express',
  'trinity_recall',
  'trinity_memorize',
  'trinity_apply_feedback',
]);

/** Group id carried in an agent's enabled/added tools. */
export const COGNITIVE_FRAMEWORK_TOOL_ID = 'trinity_cognitive';

export function isUserSelectableToolName(toolName: string): boolean {
  return !INTERNAL_GATEWAY_TOOL_NAMES.has(toolName)
    && !COGNITIVE_FRAMEWORK_TOOL_NAMES.has(toolName);
}

/** Whether a tool belongs to the cognitive framework (managed by the group switch). */
export function isCognitiveFrameworkToolName(toolName: string): boolean {
  return COGNITIVE_FRAMEWORK_TOOL_NAMES.has(toolName);
}
