/**
 * Trinity cognitive status trigger (sidebar footer).
 *
 * Just the footer trigger button now: clicking toggles the inline
 * dynamic-state panel rendered above the footer (see TrinityStatusPanel).
 * The old floating popover is gone.
 */

import React from 'react';
import { Icon, OverflowText } from '@openbitfun/ui';
import { Brain, Sparkles } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { emotionLabelKey } from '@/app/scenes/trinity/trinityDisplay';
import { useTrinityStore } from '@/app/scenes/trinity/trinityStore';

interface TrinityStatusControlProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const TrinityStatusControl: React.FC<TrinityStatusControlProps> = ({ open, onOpenChange }) => {
  const { t } = useI18n('common');
  const cognitiveState = useTrinityStore(s => s.cognitiveState);
  const status = useTrinityStore(s => s.status);

  const online = status === 'online';
  const label = online
    ? t('trinity.status.online')
    : status === 'offline'
      ? t('trinity.status.offline')
      : t('trinity.status.unknown');

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
      data-openbitfun-state={status}
    >
      <Icon glyph={online ? Sparkles : Brain} size="sm" />
      {online && cognitiveState && (
        <span
          className="openbitfun-nav-panel__being-dot"
          data-openbitfun-emotion={emotionLabelKey(cognitiveState?.emotion?.valence)}
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
