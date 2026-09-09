/**
 * Cognitive Being sidebar section.
 *
 * First-class cognitive-being block in the nav sidebar: a header row with a live
 * emotion dot plus an inline expansion holding the cognitive state summary,
 * the awakening ceremony entry, the cloud-memory entry, and a compact
 * memory-timeline preview. Expansion pushes content down (no overlay).
 */

import React, { useEffect, useState } from 'react';
import { Icon, OverflowText, Tooltip } from '@openbitfun/ui';
import { Brain, Cloud, Sparkles } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { trinityAPI } from '@/infrastructure/api';
import {
  emotionLabelKey,
  focusLabelKey,
  NEED_ATTENTION_THRESHOLD,
  NEED_KEYS,
  normalizeMemoryItems,
} from '@/app/scenes/trinity/trinityDisplay';
import { useTrinityStore } from '@/app/scenes/trinity/trinityStore';
import { useSceneStore } from '@/app/stores/sceneStore';

const PREVIEW_MEMORY_COUNT = 3;

const CognitiveBeingSection: React.FC = () => {
  const { t } = useI18n('common');
  const [expanded, setExpanded] = useState(false);
  const cognitiveState = useTrinityStore(s => s.cognitiveState);
  const status = useTrinityStore(s => s.status);
  const awakened = useTrinityStore(s => s.awakened);
  const refresh = useTrinityStore(s => s.refresh);
  const activeTabId = useSceneStore(s => s.activeTabId);
  const [memories, setMemories] = useState<ReturnType<typeof normalizeMemoryItems>>([]);
  const [awakening, setAwakening] = useState(false);

  const online = status === 'online';
  const emotion = cognitiveState?.emotion?.valence;
  const focus = cognitiveState?.focus;
  const confidence = cognitiveState?.confidence;
  const needs = cognitiveState?.needs ?? {};
  const memory = cognitiveState?.memory ?? {};
  const isTrinityActive = activeTabId === 'trinity';

  useEffect(() => {
    if (!expanded) return undefined;
    void refresh();
    let cancelled = false;
    trinityAPI
      .memoryTimeline({ limit: PREVIEW_MEMORY_COUNT })
      .then(payload => { if (!cancelled) setMemories(normalizeMemoryItems(payload)); })
      .catch(() => { if (!cancelled) setMemories([]); });
    return () => { cancelled = true; };
  }, [expanded, refresh]);

  const handleOpenPanel = () => {
    useSceneStore.getState().openScene('trinity');
  };

  const handleAwaken = async () => {
    setAwakening(true);
    try {
      await trinityAPI.awaken({
        name: t('trinity.scene.awakenName'),
        persona: 'neutral',
        user_name: t('trinity.scene.userName'),
      });
      await refresh();
    } finally {
      setAwakening(false);
    }
  };

  return (
    <div
      className="openbitfun-nav-panel__being"
      data-openbitfun-component="nav-panel"
      data-openbitfun-part="cognitiveBeing"
      data-openbitfun-emotion={emotionLabelKey(emotion)}
      data-openbitfun-need={NEED_KEYS[0]}
      data-openbitfun-state={[expanded ? 'open' : '', online ? 'online' : status].filter(Boolean).join(' ')}
    >
      <Tooltip
        content={expanded ? t('trinity.being.collapse') : t('trinity.being.expand')}
        placement="right"
        followCursor
      >
        <button
          type="button"
          className={[
            'openbitfun-nav-panel__top-action-btn',
            'openbitfun-nav-panel__being-header',
            isTrinityActive ? 'is-active' : '',
          ].filter(Boolean).join(' ')}
          onClick={() => setExpanded(value => !value)}
          aria-expanded={expanded}
          aria-label={t('trinity.being.title')}
          data-testid="nav-cognitive-being-header"
        >
          <span className="openbitfun-nav-panel__top-action-icon-slot" aria-hidden="true">
            <Icon glyph={Brain} size="sm" />
          </span>
          <OverflowText>{t('trinity.being.title')}</OverflowText>
          {online && (
            <span
              className="openbitfun-nav-panel__being-dot"
              data-openbitfun-emotion={emotionLabelKey(emotion)}
              aria-hidden="true"
            />
          )}
          <Icon
            name="chevron-down"
            size="sm"
            className={[
              'openbitfun-nav-panel__being-chevron',
              expanded ? 'is-open' : '',
            ].filter(Boolean).join(' ')}
            aria-hidden="true"
          />
        </button>
      </Tooltip>

      <div
        className={[
          'openbitfun-nav-panel__being-body',
          expanded ? 'is-open' : '',
        ].filter(Boolean).join(' ')}
        data-testid="nav-cognitive-being-body"
      >
        {!online && (
          <p className="openbitfun-nav-panel__being-offline">{t('trinity.being.offlineHint')}</p>
        )}

        {online && cognitiveState && (
          <div className="openbitfun-nav-panel__being-state" data-testid="nav-cognitive-being-state">
            <span className="openbitfun-nav-panel__being-state-line">
              <span
                className="openbitfun-nav-panel__being-dot openbitfun-nav-panel__being-dot--lg"
                data-openbitfun-emotion={emotionLabelKey(emotion)}
                aria-hidden="true"
              />
              <span className="openbitfun-nav-panel__being-emotion">
                {t(`trinity.emotion.${emotionLabelKey(emotion)}`)}
              </span>
              <span className="openbitfun-nav-panel__being-focus">
                {t(`trinity.focus.${focusLabelKey(focus)}`)}
              </span>
            </span>
            {typeof confidence === 'number' && (
              <span className="openbitfun-nav-panel__being-confidence">
                {t('trinity.status.confidence')} {Math.round(confidence * 100)}%
              </span>
            )}
            <div className="openbitfun-nav-panel__being-needs">
              {NEED_KEYS.map(key => {
                const value = needs[key];
                if (typeof value !== 'number') return null;
                const pct = Math.round(value * 100);
                return (
                  <div className="openbitfun-nav-panel__being-need" key={key}>
                    <span className="openbitfun-nav-panel__being-need-label">
                      {t(`trinity.needs.${key}`)}
                    </span>
                    <span className="openbitfun-nav-panel__being-need-track">
                      <span
                        className="openbitfun-nav-panel__being-need-fill"
                        style={{ width: `${pct}%` }}
                        data-openbitfun-need={key}
                        data-openbitfun-attention={value <= NEED_ATTENTION_THRESHOLD ? 'low' : undefined}
                      />
                    </span>
                    <span className="openbitfun-nav-panel__being-need-value">{pct}%</span>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {online && !awakened && (
          <button
            type="button"
            className="openbitfun-nav-panel__being-action"
            onClick={() => { void handleAwaken(); }}
            disabled={awakening}
            data-testid="nav-cognitive-being-awaken"
          >
            <Icon glyph={Sparkles} size="sm" />
            <OverflowText>{awakening ? t('trinity.scene.awakening') : t('trinity.being.awakenAction')}</OverflowText>
          </button>
        )}

        <div
          className="openbitfun-nav-panel__being-row"
          data-openbitfun-state="soon"
          title={t('trinity.being.comingSoon')}
          data-testid="nav-cognitive-being-cloud"
        >
          <span className="openbitfun-nav-panel__being-row-icon" aria-hidden="true">
            <Icon glyph={Cloud} size="sm" />
          </span>
          <OverflowText className="openbitfun-nav-panel__being-row-label">
            {t('trinity.being.cloudMemory')}
          </OverflowText>
          <span className="openbitfun-nav-panel__being-soon-badge">{t('trinity.being.comingSoon')}</span>
        </div>

        <div className="openbitfun-nav-panel__being-memory">
          <span className="openbitfun-nav-panel__being-memory-title">
            {t('trinity.scene.memoryTimeline')}
            {typeof memory.total_nodes === 'number' && (
              <span className="openbitfun-nav-panel__being-memory-count">
                {t('trinity.status.memoryNodes', { count: memory.total_nodes })}
              </span>
            )}
          </span>
          {memories.length === 0 ? (
            <p className="openbitfun-nav-panel__being-memory-empty">{t('trinity.scene.noMemories')}</p>
          ) : (
            <ul className="openbitfun-nav-panel__being-memory-list">
              {memories.map((entry, index) => (
                <li
                  className="openbitfun-nav-panel__being-memory-item"
                  key={entry.id ?? index}
                  title={entry.content ?? ''}
                >
                  <span className="openbitfun-nav-panel__being-memory-kind">{entry.kind ?? 'note'}</span>
                  <OverflowText className="openbitfun-nav-panel__being-memory-content">
                    {entry.content ?? ''}
                  </OverflowText>
                </li>
              ))}
            </ul>
          )}
        </div>

        <button
          type="button"
          className={[
            'openbitfun-nav-panel__being-action',
            'openbitfun-nav-panel__being-action--primary',
          ].join(' ')}
          onClick={handleOpenPanel}
          data-testid="nav-cognitive-being-open-panel"
        >
          <Icon name="arrow-up-right" size="sm" />
          <OverflowText>{t('trinity.being.openPanel')}</OverflowText>
        </button>
      </div>
    </div>
  );
};

export default CognitiveBeingSection;
