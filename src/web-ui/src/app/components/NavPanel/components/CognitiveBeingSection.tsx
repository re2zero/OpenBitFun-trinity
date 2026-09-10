/**
 * Cognitive Being sidebar entry.
 *
 * A plain nav button (no inline expansion): shows the being title with a
 * live emotion dot while awake, and opens the full cognitive console scene
 * on click. Polls at a low frequency (30s) via the shared store hook.
 */

import React from 'react';
import { Icon, OverflowText, Tooltip } from '@openbitfun/ui';
import { Brain } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { emotionLabelKey, NEED_KEYS } from '@/app/scenes/trinity/trinityDisplay';
import { useTrinityStore, useTrinityPhase, useTrinityAutoRefresh } from '@/app/scenes/trinity/trinityStore';
import { useSceneStore } from '@/app/stores/sceneStore';

/** Low-frequency refresh keeps the emotion dot roughly live. */
const STATE_REFRESH_INTERVAL_MS = 30_000;

const CognitiveBeingSection: React.FC = () => {
  const { t } = useI18n('common');
  const phase = useTrinityPhase();
  const psi = useTrinityStore(s => s.psi);
  const activeTabId = useSceneStore(s => s.activeTabId);

  useTrinityAutoRefresh(STATE_REFRESH_INTERVAL_MS);

  const emotionKey = emotionLabelKey(psi?.emotion?.valence);
  const isTrinityActive = activeTabId === 'trinity';

  return (
    <Tooltip content={t('trinity.being.title')} placement="right" followCursor>
      <button
        type="button"
        className={[
          'openbitfun-nav-panel__top-action-btn',
          'openbitfun-nav-panel__being-header',
          isTrinityActive ? 'is-active' : '',
        ].filter(Boolean).join(' ')}
        onClick={() => useSceneStore.getState().openScene('trinity')}
        aria-label={t('trinity.being.title')}
        aria-pressed={isTrinityActive}
        data-testid="nav-cognitive-being-header"
        data-openbitfun-component="nav-panel"
        data-openbitfun-part="cognitiveBeing"
        data-openbitfun-emotion={emotionKey}
        data-openbitfun-need={NEED_KEYS[0]}
        data-openbitfun-state={phase}
      >
        <span className="openbitfun-nav-panel__top-action-icon-slot" aria-hidden="true">
          <Icon glyph={Brain} size="sm" />
        </span>
        <OverflowText>{t('trinity.being.title')}</OverflowText>
        {phase === 'awake' && (
          <span
            className="openbitfun-nav-panel__being-dot"
            data-openbitfun-emotion={emotionKey}
            aria-hidden="true"
          />
        )}
      </button>
    </Tooltip>
  );
};

export default CognitiveBeingSection;
