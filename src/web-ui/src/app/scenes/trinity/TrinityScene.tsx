/**
 * Cognitive Being scene — full-surface console for the Trinity engine.
 *
 * Layout: being header (identity + daemon status + refresh) → awakening
 * ceremony card when unawakened → redesigned cognitive-state card (emotion
 * hero with per-valence hue, localized focus/confidence, five localized
 * need bars with unmet-need highlight) → cloud-memory entry → memory
 * console (stats, timeline with reinforce/forget management).
 */

import React, { useCallback, useEffect, useState } from 'react';
import { Button, Card, CardBody, CardHeader, Icon, OverflowText, ScrollArea, Spinner } from '@openbitfun/ui';
import { Brain, Cloud, RefreshCw, ShieldCheck, Sparkles, Trash2 } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { trinityAPI } from '@/infrastructure/api';
import {
  emotionLabelKey,
  focusLabelKey,
  formatMemoryTime,
  NEED_ATTENTION_THRESHOLD,
  NEED_KEYS,
  normalizeMemoryItems,
  TrinityMemoryEntry,
} from './trinityDisplay';
import { useTrinityStore } from './trinityStore';
import './TrinityScene.scss';

const TIMELINE_LIMIT = 50;
/** ms a forget button stays in its confirm state before reverting. */
const FORGET_CONFIRM_TIMEOUT_MS = 3000;

const TrinityScene: React.FC = () => {
  const { t } = useI18n('common');
  const cognitiveState = useTrinityStore(s => s.cognitiveState);
  const status = useTrinityStore(s => s.status);
  const awakened = useTrinityStore(s => s.awakened);
  const refresh = useTrinityStore(s => s.refresh);
  const [memories, setMemories] = useState<TrinityMemoryEntry[]>([]);
  const [memoriesLoading, setMemoriesLoading] = useState(false);
  const [awakening, setAwakening] = useState(false);
  const [forgetPendingId, setForgetPendingId] = useState<string | null>(null);

  const loadMemories = useCallback(async () => {
    setMemoriesLoading(true);
    try {
      const result = await trinityAPI.memoryTimeline({ limit: TIMELINE_LIMIT });
      setMemories(normalizeMemoryItems(result));
    } catch {
      setMemories([]);
    } finally {
      setMemoriesLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    void loadMemories();
  }, [loadMemories, refresh]);

  useEffect(() => {
    if (forgetPendingId == null) return undefined;
    const timer = window.setTimeout(() => setForgetPendingId(null), FORGET_CONFIRM_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [forgetPendingId]);

  const handleAwaken = useCallback(async () => {
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
  }, [refresh, t]);

  const handleReinforce = useCallback(async (id: string) => {
    try {
      await trinityAPI.reinforceMemory({ id });
      await loadMemories();
    } catch {
      /* daemon may be offline; timeline stays as-is */
    }
  }, [loadMemories]);

  const handleForget = useCallback(async (entry: TrinityMemoryEntry) => {
    const id = entry.id;
    if (id == null) return;
    // Two-step confirm: first click arms, second click within the window forgets.
    if (forgetPendingId !== id) {
      setForgetPendingId(id);
      return;
    }
    setForgetPendingId(null);
    try {
      await trinityAPI.forgetMemory({ id });
      await loadMemories();
    } catch {
      /* daemon may be offline; timeline stays as-is */
    }
  }, [forgetPendingId, loadMemories]);

  const emotion = cognitiveState?.emotion?.valence;
  const focus = cognitiveState?.focus;
  const confidence = cognitiveState?.confidence;
  const cycleCount = cognitiveState?.cycle_count;
  const needs = cognitiveState?.needs ?? {};
  const memory = cognitiveState?.memory ?? {};
  const online = status === 'online';
  const statusLabel = online
    ? t('trinity.status.online')
    : status === 'offline'
      ? t('trinity.status.offline')
      : t('trinity.status.unknown');

  return (
    <div className="openbitfun-trinity-scene" data-openbitfun-scene="trinity" data-openbitfun-part="root">
      <ScrollArea className="openbitfun-trinity-scene__scroll" data-openbitfun-scene="trinity" data-openbitfun-part="scroll">
        <div className="openbitfun-trinity-scene__content" data-openbitfun-scene="trinity" data-openbitfun-part="content">
          <header className="openbitfun-trinity-scene__header">
            <div className="openbitfun-trinity-scene__title-row">
              <Icon glyph={Brain} size="lg" />
              <h1>{t('trinity.being.title')}</h1>
              <span
                className="openbitfun-trinity-scene__daemon-badge"
                data-openbitfun-state={online ? 'online' : status}
              >
                {statusLabel}
              </span>
              <Button
                variant="outline"
                size="sm"
                leadingIcon={<Icon glyph={RefreshCw} size="sm" />}
                onClick={() => { void refresh(); void loadMemories(); }}
                data-testid="trinity-scene-refresh"
              >
                {t('trinity.scene.refresh')}
              </Button>
            </div>
            <p className="openbitfun-trinity-scene__subtitle">{t('trinity.scene.subtitle')}</p>
          </header>

          {!online && (
            <Card appearance="raised" className="openbitfun-trinity-scene__offline">
              <CardBody>
                <p>{t('trinity.status.unavailable')}</p>
              </CardBody>
            </Card>
          )}

          {online && !awakened && (
            <Card appearance="raised" className="openbitfun-trinity-scene__awaken">
              <CardBody>
                <h2>{t('trinity.scene.awakenTitle')}</h2>
                <p>{t('trinity.scene.awakenDescription')}</p>
                <Button
                  variant="primary"
                  leadingIcon={<Icon glyph={Sparkles} size="sm" />}
                  onClick={() => { void handleAwaken(); }}
                  disabled={awakening}
                  data-testid="trinity-scene-awaken"
                >
                  {awakening ? t('trinity.scene.awakening') : t('trinity.scene.awaken')}
                </Button>
              </CardBody>
            </Card>
          )}

          {online && cognitiveState && (
            <>
              <Card appearance="raised" className="openbitfun-trinity-scene__state">
                <CardHeader
                  contentAlign="center"
                  title={<h2>{t('trinity.being.state')}</h2>}
                />
                <CardBody>
                  <div className="openbitfun-trinity-scene__hero">
                    <span
                      className="openbitfun-trinity-scene__hero-dot"
                      data-openbitfun-emotion={emotionLabelKey(emotion)}
                      aria-hidden="true"
                    />
                    <div className="openbitfun-trinity-scene__hero-text">
                      <span className="openbitfun-trinity-scene__hero-emotion">
                        {t(`trinity.emotion.${emotionLabelKey(emotion)}`)}
                      </span>
                      <span className="openbitfun-trinity-scene__hero-meta">
                        {t(`trinity.focus.${focusLabelKey(focus)}`)}
                        {typeof confidence === 'number' && (
                          <> · {t('trinity.status.confidence')} {Math.round(confidence * 100)}%</>
                        )}
                        {typeof cycleCount === 'number' && (
                          <> · #{cycleCount}</>
                        )}
                      </span>
                    </div>
                  </div>
                  <div className="openbitfun-trinity-scene__needs">
                    <h3>{t('trinity.status.needs')}</h3>
                    {NEED_KEYS.map(key => {
                      const value = needs[key];
                      if (typeof value !== 'number') return null;
                      const pct = Math.round(value * 100);
                      return (
                        <div className="openbitfun-trinity-scene__need" key={key}>
                          <span className="openbitfun-trinity-scene__need-label">
                            {t(`trinity.needs.${key}`)}
                          </span>
                          <span className="openbitfun-trinity-scene__need-track">
                            <span
                              className="openbitfun-trinity-scene__need-fill"
                              style={{ width: `${pct}%` }}
                              data-openbitfun-need={key}
                              data-openbitfun-attention={value <= NEED_ATTENTION_THRESHOLD ? 'low' : undefined}
                            />
                          </span>
                          <span className="openbitfun-trinity-scene__need-value">{pct}%</span>
                        </div>
                      );
                    })}
                  </div>
                  {memory.total_nodes != null && (
                    <div className="openbitfun-trinity-scene__memory-scale">
                      <span>{t('trinity.status.memoryNodes', { count: memory.total_nodes })}</span>
                      {memory.total_triples != null && (
                        <span> · {t('trinity.status.memoryTriples', { count: memory.total_triples })}</span>
                      )}
                      {memory.vocabulary_size != null && (
                        <span> · {t('trinity.status.vocabulary', { count: memory.vocabulary_size })}</span>
                      )}
                    </div>
                  )}
                </CardBody>
              </Card>

              <div className="openbitfun-trinity-scene__cloud" data-openbitfun-state="soon">
                <span className="openbitfun-trinity-scene__cloud-icon" aria-hidden="true">
                  <Icon glyph={Cloud} size="sm" />
                </span>
                <span className="openbitfun-trinity-scene__cloud-label">{t('trinity.being.cloudMemory')}</span>
                <span className="openbitfun-trinity-scene__cloud-hint">{t('trinity.being.cloudMemoryHint')}</span>
                <span className="openbitfun-trinity-scene__soon-badge">{t('trinity.being.comingSoon')}</span>
              </div>

              <Card appearance="raised" className="openbitfun-trinity-scene__timeline">
                <CardHeader
                  contentAlign="center"
                  title={<h2>{t('trinity.scene.memoryTimeline')}</h2>}
                />
                <CardBody>
                  {memoriesLoading ? (
                    <Spinner size="md" />
                  ) : memories.length === 0 ? (
                    <p className="openbitfun-trinity-scene__empty">{t('trinity.scene.noMemories')}</p>
                  ) : (
                    <ul className="openbitfun-trinity-scene__memory-list">
                      {memories.map((entry, index) => {
                        const time = formatMemoryTime(entry);
                        const forgetArmed = entry.id != null && forgetPendingId === entry.id;
                        return (
                          <li className="openbitfun-trinity-scene__memory-item" key={entry.id ?? index}>
                            <span className="openbitfun-trinity-scene__memory-kind">{entry.kind ?? 'note'}</span>
                            <OverflowText className="openbitfun-trinity-scene__memory-content">
                              {entry.content}
                            </OverflowText>
                            {entry.project && (
                              <span className="openbitfun-trinity-scene__memory-project">{entry.project}</span>
                            )}
                            {time && (
                              <span className="openbitfun-trinity-scene__memory-time">{time}</span>
                            )}
                            {entry.id != null && (
                              <span className="openbitfun-trinity-scene__memory-actions">
                                <button
                                  type="button"
                                  className="openbitfun-trinity-scene__memory-action"
                                  onClick={() => { void handleReinforce(entry.id as string); }}
                                  aria-label={t('trinity.memory.reinforce')}
                                  title={t('trinity.memory.reinforce')}
                                  data-testid="trinity-memory-reinforce"
                                >
                                  <Icon glyph={ShieldCheck} size="sm" />
                                </button>
                                <button
                                  type="button"
                                  className={[
                                    'openbitfun-trinity-scene__memory-action',
                                    forgetArmed ? 'is-armed' : '',
                                  ].filter(Boolean).join(' ')}
                                  onClick={() => { void handleForget(entry); }}
                                  aria-label={forgetArmed ? t('trinity.memory.forgetConfirm') : t('trinity.memory.forget')}
                                  title={forgetArmed ? t('trinity.memory.forgetConfirm') : t('trinity.memory.forget')}
                                  data-testid="trinity-memory-forget"
                                >
                                  <Icon glyph={Trash2} size="sm" />
                                </button>
                              </span>
                            )}
                          </li>
                        );
                      })}
                    </ul>
                  )}
                </CardBody>
              </Card>
            </>
          )}
        </div>
      </ScrollArea>
    </div>
  );
};

export default TrinityScene;
