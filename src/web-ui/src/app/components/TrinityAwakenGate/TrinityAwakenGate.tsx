/**
 * Trinity awakening gate.
 *
 * Watches the active workspace: when it is the Trinity assistant and the
 * being is in the dormant phase (engine connected, ceremony not yet done),
 * the awakening ceremony dialog surfaces automatically. Once awakened the
 * phase flips to awake and the gate closes for good.
 */

import React, { useEffect, useState } from 'react';
import { useWorkspaceContext } from '@/infrastructure/contexts/WorkspaceContext';
import { useTrinityStore, useTrinityPhase } from '@/app/scenes/trinity/trinityStore';
import TrinityAwakenDialog from './TrinityAwakenDialog';

const TrinityAwakenGate: React.FC = () => {
  const { currentWorkspace } = useWorkspaceContext();
  const phase = useTrinityPhase();
  const refresh = useTrinityStore(s => s.refresh);
  const [open, setOpen] = useState(false);

  const isTrinityWorkspace = currentWorkspace?.assistantId === 'trinity';

  // One initial probe so the phase is known before gating.
  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    setOpen(isTrinityWorkspace && phase === 'dormant');
  }, [isTrinityWorkspace, phase]);

  return <TrinityAwakenDialog open={open} onClose={() => setOpen(false)} workspacePath={currentWorkspace?.rootPath} />;
};

export default TrinityAwakenGate;
