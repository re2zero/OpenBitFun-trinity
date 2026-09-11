/**
 * Visibility predicate for the cognitive-being identity recovery action.
 *
 * The action is offered only when the engine is awake AND the cognitive being
 * has no usable identity in the workspace:
 * - no Trinity assistant workspace exists at all, or
 * - the primary assistant role is held by a different workspace, or
 * - the Trinity workspace exists but was reset back to the generic templates
 *   (its IDENTITY.md has no parsed name).
 *
 * Offline / not-yet-awakened engines never show it: there is no awakened
 * identity to restore from.
 */
import type { TrinityPhase } from './trinityStore';

/** Assistant id of the Trinity cognitive being workspace. */
export const COGNITIVE_BEING_ASSISTANT_ID = 'trinity';

export interface CognitiveIdentityWorkspace {
  id: string;
  assistantId?: string | null;
  identity?: { name?: string | null } | null;
}

export function needsCognitiveIdentity(
  phase: TrinityPhase,
  assistantWorkspaces: readonly CognitiveIdentityWorkspace[],
  primaryAssistantWorkspaceId: string | null | undefined,
): boolean {
  if (phase !== 'awake') {
    return false;
  }

  const workspace = assistantWorkspaces.find(
    (candidate) => candidate.assistantId === COGNITIVE_BEING_ASSISTANT_ID,
  );
  if (!workspace) {
    return true;
  }

  if (workspace.id !== primaryAssistantWorkspaceId) {
    return true;
  }

  return !workspace.identity?.name?.trim();
}
