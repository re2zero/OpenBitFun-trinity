/**
 * Trinity awakening gate.
 *
 * First launch: the factory being ships un-awakened, so the dormant phase
 * (engine connected, ceremony not yet done) means the onboarding wizard has
 * never run. The wizard surfaces full-screen wherever the user is — no
 * workspace precondition — and once awakened the phase flips to awake and
 * the gate closes for good.
 */

import React, { useEffect, useState } from 'react';
import { useTrinityStore, useTrinityPhase } from '@/app/scenes/trinity/trinityStore';
import OnboardingWizard from '@/app/scenes/onboarding/OnboardingWizard';

const TrinityAwakenGate: React.FC = () => {
  const phase = useTrinityPhase();
  const refresh = useTrinityStore(s => s.refresh);
  const [open, setOpen] = useState(false);

  // One initial probe so the phase is known before gating.
  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (phase === 'dormant') setOpen(true);
  }, [phase]);

  return <OnboardingWizard open={open} onClose={() => setOpen(false)} />;
};

export default TrinityAwakenGate;
