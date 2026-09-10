/**
 * Trinity cognitive status trigger (sidebar footer).
 *
 * The footer "lamp": phase label (disconnected / dormant / awake) plus the
 * live emotion dot while awake. Clicking toggles the inline quick panel
 * rendered above the footer (see TrinityStatusPanel).
 */

import React from 'react';
import { Icon, OverflowText } from '@openbitfun/ui';
import { Brain } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { emotionLabelKey } from '@/app/scenes/trinity/trinityDisplay';
import { useTrinityStore, useTrinityPhase } from '@/app/scenes/trinity/trinityStore';

interface TrinityStatusControlProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const TrinityStatusControl: React.FC<TrinityStatusControlProps> = ({ open, onOpenChange }) => {
  const { t } = useI18n('common');
  const phase = useTrinityPhase();
  const psi = useTrinityStore(s => s.psi);

  const label = t(`trinity.phase.${phase}`);

  return (
    <button
      data-overflow-trigger
      type="button"
      className={`openbitfun-nav-panel__footer-device-status${open ? ' is-open' : ''}`}
      aria-label={label}
      aria-expanded={open}
      onClick={() => onOpenChange(!open)}
      data-testid="nav-footer-trinity-status"
      data-openbitfun-component="nav-panel"
      data-openbitfun-part="trinityStatus"
      data-openbitfun-state={phase}
    >
      {phase === 'awake' ? <Icon name="spark" size="sm" /> : <Icon glyph={Brain} size="sm" />}
      {phase === 'awake' && psi && (
        <span
          className="openbitfun-nav-panel__being-dot"
          data-openbitfun-emotion={emotionLabelKey(psi?.emotion?.valence)}
          aria-hidden="true"
        />
      )}
      <OverflowText className="openbitfun-nav-panel__footer-device-status-label">
        {label}
      </OverflowText>
    </button>
  );
};

export default TrinityStatusControl;
