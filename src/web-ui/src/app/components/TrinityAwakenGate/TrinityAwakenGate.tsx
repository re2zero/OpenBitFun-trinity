/**
 * Trinity awakening gate.
 *
 * Watches the active workspace: when it is the Trinity assistant and the
 * cognitive engine is online but the being is not awakened yet, the first
 * conversation surfaces the awakening ceremony dialog.
 */

import React, { useEffect, useState } from 'react';
import { useWorkspaceContext } from '@/infrastructure/contexts/WorkspaceContext';
import { useTrinityStore } from '@/app/scenes/trinity/trinityStore';
import TrinityAwakenDialog from './TrinityAwakenDialog';

const TrinityAwakenGate: React.FC = () => {
  const { currentWorkspace } = useWorkspaceContext();
  const status = useTrinityStore(s => s.status);
  const awakened = useTrinityStore(s => s.awakened);
  const refresh = useTrinityStore(s => s.refresh);
  const [open, setOpen] = useState(false);

  const isTrinityWorkspace = currentWorkspace?.assistantId === 'trinity';

  useEffect(() => {
    if (!isTrinityWorkspace) {
      setOpen(false);
      return;
    }
    if (status === 'unknown') {
      void refresh();
      return;
    }
    if (status === 'online' && !awakened) {
      setOpen(true);
    }
  }, [awakened, isTrinityWorkspace, refresh, status]);

  return <TrinityAwakenDialog open={open} onClose={() => setOpen(false)} workspacePath={currentWorkspace?.rootPath} />;
};

export default TrinityAwakenGate;