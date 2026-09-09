/**
 * Inline dynamic cognitive-state panel anchored above the sidebar footer.
 *
 * Expands upward from the Trinity footer trigger by pushing sidebar content
 * up (flex sibling of the footer, no overlay/popover). While open it refresh
 *es immediately and keeps polling so the state stays live; collapsing stops
 * the poll. Escape also collapses.
 */

import React, { useEffect } from 'react';
import { Button, Icon, OverflowText } from '@openbitfun/ui';
import { Sparkles, X } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { trinityAPI } from '@/infrastructure/api';
import {
  emotionLabelKey,
  focusLabelKey,
  NEED_ATTENTION_THRESHOLD,
  NEED_KEYS,
} from '@/app/scenes/trinity/trinityDisplay';
import { useTrinityStore } from '@/app/scenes/trinity/trinityStore';
import { useSceneStore } from '@/app/stores/sceneStore';

const POLL_INTERVAL_MS = 5000;

interface TrinityStatusPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const TrinityStatusPanel: React.FC<TrinityStatusPanelProps> = ({ open, onOpenChange }) => {
  const { t } = useI18n('common');
  const cognitiveState = useTrinityStore(s => s.cognitiveState);
  const status = useTrinityStore(s => s.status);
  const awakened = useTrinityStore(s => s.awakened);
  const refresh = useTrinityStore(s => s.refresh);

  const online = status === 'online';
  const emotion = cognitiveState?.emotion?.valence;
  const focus = cognitiveState?.focus;
  const confidence = cognitiveState?.confidence;
  const needs = cognitiveState?.needs ?? {};
  const memory = cognitiveState?.memory ?? {};

  useEffect(() => {
    if (!open) return undefined;
    void refresh();
    const interval = window.setInterval(() => { void refresh(); }, POLL_INTERVAL_MS);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onOpenChange(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [onOpenChange, open, refresh]);

  const handleOpenPanel = () => {
    onOpenChange(false);
    useSceneStore.getState().openScene('trinity');
  };

  const handleAwaken = async () => {
    try {
      await trinityAPI.awaken({
        name: t('trinity.scene.awakenName'),
        persona: 'neutral',
        user_name: t('trinity.scene.userName'),
      });
      await refresh();
    } catch {
      /* daemon offline — panel already shows the offline hint */
    }
  };

  return (
    <div
      className={[
        'openbitfun-nav-panel__trinity-panel',
        open ? 'is-open' : '',
      ].filter(Boolean).join(' ')}
      data-openbitfun-component="nav-panel"
      data-openbitfun-part="trinityPanel"
      data-openbitfun-emotion={emotionLabelKey(emotion)}
      data-openbitfun-need={NEED_KEYS[0]}
      data-openbitfun-state={[open ? 'open' : '', online ? 'online' : status].filter(Boolean).join(' ')}
      aria-hidden={!open}
      data-testid="nav-trinity-status-panel"
    >
      <div className="openbitfun-nav-panel__trinity-panel-inner">
        <div className="openbitfun-nav-panel__trinity-panel-header">
          <span className="openbitfun-nav-panel__trinity-panel-title">
            {t('trinity.status.title')}
          </span>
          <button
            type="button"
            className="openbitfun-nav-panel__trinity-panel-close"
            onClick={() => onOpenChange(false)}
            aria-label={t('trinity.being.collapse')}
            data-testid="nav-trinity-status-collapse"
          >
            <Icon glyph={X} size="sm" />
          </button>
        </div>

        {!online && (
          <p className="openbitfun-nav-panel__trinity-panel-offline">
            {t('trinity.being.offlineHint')}
          </p>
        )}

        {online && cognitiveState && (
          <>
            <div className="openbitfun-nav-panel__trinity-panel-hero">
              <span
                className="openbitfun-nav-panel__being-dot openbitfun-nav-panel__being-dot--lg"
                data-openbitfun-emotion={emotionLabelKey(emotion)}
                aria-hidden="true"
              />
              <span className="openbitfun-nav-panel__trinity-panel-emotion">
                {t(`trinity.emotion.${emotionLabelKey(emotion)}`)}
              </span>
              <span className="openbitfun-nav-panel__trinity-panel-focus">
                {t(`trinity.focus.${focusLabelKey(focus)}`)}
              </span>
              {typeof confidence === 'number' && (
                <span className="openbitfun-nav-panel__trinity-panel-confidence">
                  {t('trinity.status.confidence')} {Math.round(confidence * 100)}%
                </span>
              )}
            </div>

            <div className="openbitfun-nav-panel__trinity-panel-needs">
              {NEED_KEYS.map(key => {
                const value = needs[key];
                if (typeof value !== 'number') return null;
                const pct = Math.round(value * 100);
                return (
                  <div className="openbitfun-nav-panel__trinity-panel-need" key={key}>
                    <span className="openbitfun-nav-panel__trinity-panel-need-label">
                      {t(`trinity.needs.${key}`)}
                    </span>
                    <span className="openbitfun-nav-panel__trinity-panel-need-track">
                      <span
                        className="openbitfun-nav-panel__trinity-panel-need-fill"
                        style={{ width: `${pct}%` }}
                        data-openbitfun-need={key}
                        data-openbitfun-attention={value <= NEED_ATTENTION_THRESHOLD ? 'low' : undefined}
                      />
                    </span>
                    <span className="openbitfun-nav-panel__trinity-panel-need-value">{pct}%</span>
                  </div>
                );
              })}
            </div>

            {typeof memory.total_nodes === 'number' && (
              <div className="openbitfun-nav-panel__trinity-panel-memory">
                {t('trinity.status.memoryNodes', { count: memory.total_nodes })}
                {typeof memory.total_triples === 'number' && (
                  <span> · {t('trinity.status.memoryTriples', { count: memory.total_triples })}</span>
                )}
              </div>
            )}

            {!awakened && (
              <Button
                className="openbitfun-nav-panel__trinity-panel-awaken"
                variant="outline"
                size="sm"
                leadingIcon={<Icon glyph={Sparkles} size="sm" />}
                onClick={() => { void handleAwaken(); }}
                data-testid="nav-trinity-panel-awaken"
              >
                {t('trinity.being.awakenAction')}
              </Button>
            )}
          </>
        )}

        <Button
          className="openbitfun-nav-panel__trinity-panel-open"
          variant="text"
          size="sm"
          onClick={handleOpenPanel}
          data-testid="nav-trinity-panel-open-full"
        >
          <OverflowText>{t('trinity.being.openPanel')}</OverflowText>
        </Button>
      </div>
    </div>
  );
};

export default TrinityStatusPanel;
