/**
 * Inline cognitive quick panel anchored above the sidebar footer.
 *
 * Phase-aware quick glance (see docs/bitfun-trinity-cognitive-ui-design.md):
 * - offline: identity header + "engine disconnected" hint only
 * - dormant: awakening guidance + single CTA that opens the shared
 *   TrinityAwakenDialog (no inline ceremony logic)
 * - awake:   emotion hero + need bars (percentages at one decimal place)
 *            + cloud-sync glance line + "open full panel" action
 *
 * Expands upward from the Trinity footer trigger by pushing sidebar content
 * up (flex sibling of the footer, no overlay/popover). While open it polls
 * via the shared store (5s) and pulls cloud status once; collapsing stops
 * the poll. Escape also collapses.
 */

import React, { useEffect, useState } from 'react';
import { Button, Icon, OverflowText } from '@openbitfun/ui';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { useSceneStore } from '@/app/stores/sceneStore';
import { useTrinityStore, useTrinityPhase, useTrinityAutoRefresh } from '@/app/scenes/trinity/trinityStore';
import {
  emotionLabelKey,
  focusLabelKey,
  formatPercent,
  NEED_ATTENTION_THRESHOLD,
  NEED_KEYS,
} from '@/app/scenes/trinity/trinityDisplay';
import TrinityAwakenDialog from '@/app/components/TrinityAwakenGate/TrinityAwakenDialog';

/** Poll interval while the panel is open; closed panel polls nowhere. */
const OPEN_POLL_INTERVAL_MS = 5000;

interface TrinityStatusPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const TrinityStatusPanel: React.FC<TrinityStatusPanelProps> = ({ open, onOpenChange }) => {
  const { t } = useI18n('common');
  const phase = useTrinityPhase();
  const psi = useTrinityStore(s => s.psi);
  const identity = useTrinityStore(s => s.identity);
  const cloudStatus = useTrinityStore(s => s.cloudStatus);
  const loadCloud = useTrinityStore(s => s.loadCloud);

  const [awakenDialogOpen, setAwakenDialogOpen] = useState(false);

  useTrinityAutoRefresh(open ? OPEN_POLL_INTERVAL_MS : null);

  useEffect(() => {
    if (!open) return undefined;
    void loadCloud();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onOpenChange(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [loadCloud, onOpenChange, open]);

  const emotion = psi?.emotion?.valence;
  const focus = psi?.focus;
  const confidence = psi?.confidence;
  const needs = psi?.needs ?? {};
  const title = identity?.name || t('trinity.being.title');
  const cloudReady = cloudStatus?.registered === true && cloudStatus?.key_ready === true;

  const handleOpenPanel = () => {
    onOpenChange(false);
    useSceneStore.getState().openScene('trinity');
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
      data-openbitfun-state={[open ? 'open' : '', phase].filter(Boolean).join(' ')}
      aria-hidden={!open}
      data-testid="nav-trinity-status-panel"
    >
      <div className="openbitfun-nav-panel__trinity-panel-inner">
        <div className="openbitfun-nav-panel__trinity-panel-header">
          <span className="openbitfun-nav-panel__trinity-panel-title">
            {title}
            <span
              className="openbitfun-nav-panel__trinity-panel-phase"
              data-openbitfun-state={phase}
            >
              {t(`trinity.phase.${phase}`)}
            </span>
          </span>
          <button
            type="button"
            className="openbitfun-nav-panel__trinity-panel-close"
            onClick={() => onOpenChange(false)}
            aria-label={t('trinity.being.collapse')}
            data-testid="nav-trinity-status-collapse"
          >
            <Icon name="xmark" size="sm" />
          </button>
        </div>

        {phase === 'offline' && (
          <p className="openbitfun-nav-panel__trinity-panel-offline">
            {t('trinity.being.offlineHint')}
          </p>
        )}

        {phase === 'dormant' && (
          <div className="openbitfun-nav-panel__trinity-panel-dormant">
            <p>{t('trinity.scene.awakenDescription')}</p>
            <Button
              variant="outline"
              size="sm"
              leadingIcon={<Icon name="spark" size="sm" />}
              onClick={() => setAwakenDialogOpen(true)}
              data-testid="nav-trinity-panel-awaken"
            >
              {t('trinity.scene.awaken')}
            </Button>
          </div>
        )}

        {phase === 'awake' && psi && (
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
                  {t('trinity.status.confidence')} {formatPercent(confidence)}
                </span>
              )}
            </div>

            <div className="openbitfun-nav-panel__trinity-panel-needs">
              {NEED_KEYS.map(key => {
                const value = needs[key];
                if (typeof value !== 'number') return null;
                return (
                  <div className="openbitfun-nav-panel__trinity-panel-need" key={key}>
                    <span className="openbitfun-nav-panel__trinity-panel-need-label">
                      {t(`trinity.needs.${key}`)}
                    </span>
                    <span className="openbitfun-nav-panel__trinity-panel-need-track">
                      <span
                        className="openbitfun-nav-panel__trinity-panel-need-fill"
                        style={{ width: `${value * 100}%` }}
                        data-openbitfun-need={key}
                        data-openbitfun-attention={value <= NEED_ATTENTION_THRESHOLD ? 'low' : undefined}
                      />
                    </span>
                    <span className="openbitfun-nav-panel__trinity-panel-need-value">
                      {formatPercent(value)}
                    </span>
                  </div>
                );
              })}
            </div>

            {cloudReady && (
              <div className="openbitfun-nav-panel__trinity-panel-cloud">
                {cloudStatus?.user_id && <span>{cloudStatus.user_id}</span>}
                {typeof cloudStatus?.pending_ops === 'number' && (
                  <span> · {t('trinity.cloud.pendingOps', { count: cloudStatus.pending_ops })}</span>
                )}
                <span data-openbitfun-state={cloudStatus?.engine_running ? 'online' : 'offline'}>
                  {' '}· {cloudStatus?.engine_running
                    ? t('trinity.cloud.engineRunning')
                    : t('trinity.cloud.engineStopped')}
                </span>
              </div>
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

      <TrinityAwakenDialog
        open={awakenDialogOpen}
        onClose={() => setAwakenDialogOpen(false)}
      />
    </div>
  );
};

export default TrinityStatusPanel;
