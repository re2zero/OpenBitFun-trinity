/**
 * Cognitive Being scene — full-surface console for the Trinity engine.
 *
 * Phase-gated (see the Trinity repo cognitive UI design doc):
 * - offline: identity header + "engine disconnected" hint only
 * - dormant: awakening card only (opens the shared TrinityAwakenDialog) —
 *   memory and cloud consoles are meaningless before the being exists
 * - awake:   cloud-memory card first (the action surface: signup/login → key
 *   ceremony → sync/backup/restore), then the cognitive-state card (emotion
 *   hero, need bars colored per need state at one decimal place, valence trend
 *   sparkline), then the memory console (search, manual add, cursor-paginated
 *   full-text timeline with reinforce/forget management). The header carries a
 *   cognitive-framework chip; the framework's tool list lives with its switch
 *   in the assistant defaults page, not in this column.
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Card, CardBody, CardHeader, Icon, OverflowText, ScrollArea, Spinner } from '@openbitfun/ui';
import { Brain, CloudUpload, ShieldCheck } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { useWorkspaceContext } from '@/infrastructure/contexts/WorkspaceContext';
import { trinityAPI, workspaceAPI } from '@/infrastructure/api';
import { confirmDanger } from '@/infrastructure/confirm-dialog';
import TrinityAwakenDialog from '@/app/components/TrinityAwakenGate/TrinityAwakenDialog';
import { flowChatManager } from '@/flow_chat/services/FlowChatManager';
import { openMainSession } from '@/flow_chat/services/sessionActivation';
import { flowChatSessionConfigForWorkspace } from '@/app/utils/projectSessionWorkspace';
import { buildCognitiveIdentityFiles } from './cognitiveIdentityTemplate';
import { needsCognitiveIdentity } from './cognitiveIdentityStatus';
import {
  emotionLabelKey,
  focusLabelKey,
  formatMemoryTime,
  formatPercent,
  NEED_KEYS,
  needState,
  normalizeMemoryItems,
  personaLabel,
  TrinityMemoryEntry,
  valenceScore,
} from './trinityDisplay';
import {
  useTrinityStore,
  useTrinityPhase,
  useTrinityAutoRefresh,
  TrinityCloudStatus,
} from './trinityStore';
import type { CognitiveFrameworkInfo } from '@/infrastructure/api/service-api/TrinityAPI';
import './TrinityScene.scss';

const TIMELINE_LIMIT = 50;
const HISTORY_LIMIT = 120;
/** Poll while the scene is open. */
const SCENE_POLL_INTERVAL_MS = 5000;
/** ms a forget button stays in its confirm state before reverting. */
const FORGET_CONFIRM_TIMEOUT_MS = 3000;
/** Assistant workspaces chat through Claw sessions, matching the sidebar action. */
const ASSISTANT_SESSION_MODE = 'Claw';

/** One `cognition_history` point, as the daemon reports it. */
interface HistoryPoint {
  ts?: number;
  cycle?: number;
  emotion?: { valence?: string; arousal?: number; dominance?: number };
}

/** Minimal inline sparkline; hides itself when there is nothing to draw. */
const TrendSparkline: React.FC<{ points: number[] }> = ({ points }) => {
  if (points.length < 2) return null;
  const min = Math.min(...points);
  const max = Math.max(...points);
  const range = max - min || 1;
  const width = 100;
  const height = 32;
  const step = width / (points.length - 1);
  const path = points
    .map((v, i) => {
      const x = i * step;
      const y = height - 2 - ((v - min) / range) * (height - 4);
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(' ');
  return (
    <svg
      className="openbitfun-trinity-scene__trend-svg"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <polyline
        points={path}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
};

/**
 * Drops the generic bootstrap prompt — the cognitive being's identity replaces
 * it as the first-conversation gate. A missing file is not an error.
 */
async function removeBootstrapFile(workspaceRoot: string): Promise<void> {
  const base = workspaceRoot.replace(/[\\/]+$/, '');
  try {
    await workspaceAPI.deleteFile(`${base}/BOOTSTRAP.md`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (!/does not exist|no such file|not found/i.test(message)) {
      throw error;
    }
  }
}

// ── Cloud sync card (three-step sub-machine, awake only) ────────

type CloudAuthMode = 'signup' | 'login';

interface CloudCardProps {
  cloudStatus: TrinityCloudStatus | null;
  busy: string | null;
  message: string | null;
  onAction: (key: string, action: () => Promise<unknown>) => Promise<void>;
  onAfterRestore: () => Promise<void>;
}

const CloudCard: React.FC<CloudCardProps> = ({ cloudStatus, busy, message, onAction, onAfterRestore }) => {
  const { t } = useI18n('common');
  const [mode, setMode] = useState<CloudAuthMode>('signup');
  const [userId, setUserId] = useState('');
  const [password, setPassword] = useState('');
  const [passphrase, setPassphrase] = useState('');

  const registered = cloudStatus?.registered === true;
  const keyReady = cloudStatus?.key_ready === true;
  const disabled = busy != null;

  const submitAuth = (event: React.FormEvent) => {
    event.preventDefault();
    if (!userId.trim() || !password || disabled) return;
    const payload = { user_id: userId.trim(), password, device_name: 'desktop' };
    void onAction(mode, () => (mode === 'signup'
      ? trinityAPI.cloudSignup(payload)
      : trinityAPI.cloudLogin(payload)));
  };

  const submitKey = (event: React.FormEvent) => {
    event.preventDefault();
    if (passphrase.length < 8 || disabled) return;
    void onAction('setup_key', () => trinityAPI.cloudSetupKey({ passphrase }));
  };

  return (
    <Card appearance="raised" padding="md" gap="sm" className="openbitfun-trinity-scene__cloud">
      <CardHeader
        contentAlign="center"
        title={<h2>{t('trinity.cloud.title')}</h2>}
      />
      <CardBody>
        <p className="openbitfun-trinity-scene__cloud-desc">{t('trinity.cloud.desc')}</p>
        {cloudStatus == null ? (
          <p className="openbitfun-trinity-scene__cloud-unavailable">
            {t('trinity.cloud.unavailable')}
          </p>
        ) : !registered ? (
          <form className="openbitfun-trinity-scene__cloud-form" onSubmit={submitAuth}>
            <label className="openbitfun-trinity-scene__cloud-field">
              <span>{t('trinity.cloud.userId')}</span>
              <input
                className="openbitfun-config-input"
                type="text"
                value={userId}
                autoComplete="username"
                onChange={(event) => setUserId(event.target.value)}
                data-testid="trinity-cloud-user-id"
              />
            </label>
            <label className="openbitfun-trinity-scene__cloud-field">
              <span>{t('trinity.cloud.password')}</span>
              <input
                className="openbitfun-config-input"
                type="password"
                value={password}
                autoComplete={mode === 'signup' ? 'new-password' : 'current-password'}
                onChange={(event) => setPassword(event.target.value)}
                data-testid="trinity-cloud-password"
              />
            </label>
            <div className="openbitfun-trinity-scene__cloud-actions">
              <Button variant="primary" size="sm" type="submit" disabled={disabled || !userId.trim() || !password}>
                {mode === 'signup' ? t('trinity.cloud.signup') : t('trinity.cloud.login')}
              </Button>
              <Button
                variant="text"
                size="sm"
                type="button"
                onClick={() => setMode(m => (m === 'signup' ? 'login' : 'signup'))}
              >
                {mode === 'signup' ? t('trinity.cloud.toLogin') : t('trinity.cloud.toSignup')}
              </Button>
            </div>
          </form>
        ) : !keyReady ? (
          <form className="openbitfun-trinity-scene__cloud-form" onSubmit={submitKey}>
            <p className="openbitfun-trinity-scene__cloud-warning">{t('trinity.cloud.keyDesc')}</p>
            <label className="openbitfun-trinity-scene__cloud-field">
              <span>{t('trinity.cloud.passphrase')}</span>
              <input
                className="openbitfun-config-input"
                type="password"
                value={passphrase}
                minLength={8}
                autoComplete="new-password"
                onChange={(event) => setPassphrase(event.target.value)}
                data-testid="trinity-cloud-passphrase"
              />
            </label>
            <div className="openbitfun-trinity-scene__cloud-actions">
              <Button
                variant="primary"
                size="sm"
                type="submit"
                disabled={disabled || passphrase.length < 8}
                data-testid="trinity-cloud-setup-key"
              >
                {t('trinity.cloud.setupKey')}
              </Button>
            </div>
          </form>
        ) : (
          <div className="openbitfun-trinity-scene__cloud-ready">
            <div className="openbitfun-trinity-scene__cloud-status-lines">
              {cloudStatus.user_id && (
                <span>{t('trinity.cloud.userId')}: {cloudStatus.user_id}</span>
              )}
              {typeof cloudStatus.pending_ops === 'number' && (
                <span>{t('trinity.cloud.pendingOps', { count: cloudStatus.pending_ops })}</span>
              )}
              <span data-openbitfun-state={cloudStatus.engine_running ? 'online' : 'offline'}>
                {cloudStatus.engine_running
                  ? t('trinity.cloud.engineRunning')
                  : t('trinity.cloud.engineStopped')}
              </span>
            </div>
            <div className="openbitfun-trinity-scene__cloud-actions">
              <Button
                variant="outline"
                size="sm"
                disabled={disabled}
                onClick={() => { void onAction('sync_now', () => trinityAPI.cloudSyncNow()); }}
                data-testid="trinity-cloud-sync-now"
              >
                {t('trinity.cloud.syncNow')}
              </Button>
              <Button
                variant="outline"
                size="sm"
                leadingIcon={<Icon glyph={CloudUpload} size="sm" />}
                disabled={disabled}
                onClick={() => { void onAction('backup', () => trinityAPI.cloudBackup()); }}
                data-testid="trinity-cloud-backup"
              >
                {t('trinity.cloud.backup')}
              </Button>
              <Button
                variant="outline"
                size="sm"
                leadingIcon={<Icon name="arrow-down" size="sm" />}
                disabled={disabled}
                onClick={() => { void onAction('restore', async () => {
                  const result = await trinityAPI.cloudRestore();
                  await onAfterRestore();
                  return result;
                }); }}
                data-testid="trinity-cloud-restore"
              >
                {t('trinity.cloud.restore')}
              </Button>
            </div>
          </div>
        )}
        {busy != null && <Spinner size="sm" />}
        {message && (
          <p className="openbitfun-trinity-scene__cloud-message">{message}</p>
        )}
      </CardBody>
    </Card>
  );
};

const TrinityScene: React.FC = () => {
  const { t } = useI18n('common');
  const {
    currentWorkspace,
    assistantWorkspacesList,
    primaryAssistantWorkspaceId,
    ensureCognitiveBeingAssistant,
    setPrimaryAssistantWorkspace,
  } = useWorkspaceContext();
  const phase = useTrinityPhase();
  const psi = useTrinityStore(s => s.psi);
  const identity = useTrinityStore(s => s.identity);
  const cloudStatus = useTrinityStore(s => s.cloudStatus);
  const loadCloud = useTrinityStore(s => s.loadCloud);

  const [memories, setMemories] = useState<TrinityMemoryEntry[]>([]);
  const [memoriesLoading, setMemoriesLoading] = useState(false);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [timelineExhausted, setTimelineExhausted] = useState(false);
  const [history, setHistory] = useState<HistoryPoint[]>([]);
  const [cloudBusy, setCloudBusy] = useState<string | null>(null);
  const [cloudMessage, setCloudMessage] = useState<string | null>(null);
  const [framework, setFramework] = useState<CognitiveFrameworkInfo | null>(null);

  const [awakenDialogOpen, setAwakenDialogOpen] = useState(false);
  const [forgetPendingId, setForgetPendingId] = useState<string | null>(null);
  const [creatingIdentity, setCreatingIdentity] = useState(false);
  const [createIdentityError, setCreateIdentityError] = useState<string | null>(null);

  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<TrinityMemoryEntry[] | null>(null);
  const [searching, setSearching] = useState(false);

  const [addOpen, setAddOpen] = useState(false);
  const [addContent, setAddContent] = useState('');
  const [addProject, setAddProject] = useState('');
  const [adding, setAdding] = useState(false);

  const emotion = psi?.emotion?.valence;
  const focus = psi?.focus;
  const confidence = psi?.confidence;
  const cycleCount = psi?.cycle_count;
  const needs = psi?.needs ?? {};
  const memory = psi?.memory ?? {};
  // Identity is the engine-owned being record (`beings/core`, written by the
  // daemon) when it is readable; the daemon status payload is only the fallback
  // for installs whose being record predates the registry.
  const being = framework?.being ?? null;
  const identityName = being?.name || identity?.name || t('trinity.being.title');
  const beingOwner = being?.userName || identity?.user_name;
  const persona = being?.persona || identity?.persona;
  // Persona files only make sense inside the Trinity assistant workspace.
  const awakenWorkspacePath = currentWorkspace?.assistantId === 'trinity'
    ? currentWorkspace.rootPath
    : undefined;
  const shouldCreateIdentity = needsCognitiveIdentity(
    phase,
    assistantWorkspacesList,
    primaryAssistantWorkspaceId,
  );

  useTrinityAutoRefresh(SCENE_POLL_INTERVAL_MS);

  const loadMemories = useCallback(async () => {
    setMemoriesLoading(true);
    setTimelineExhausted(false);
    try {
      const result = await trinityAPI.memoryTimeline({ limit: TIMELINE_LIMIT, full: true });
      setMemories(normalizeMemoryItems(result));
    } catch {
      setMemories([]);
    } finally {
      setMemoriesLoading(false);
    }
  }, []);

  const loadHistory = useCallback(async () => {
    try {
      const result = await trinityAPI.cognitionHistory({ limit: HISTORY_LIMIT });
      const points = Array.isArray(result?.points) ? (result.points as HistoryPoint[]) : [];
      setHistory(points);
    } catch {
      setHistory([]);
    }
  }, []);

  // Cognitive framework group: registered tools + the core being identity.
  const loadFramework = useCallback(async () => {
    setFramework(await trinityAPI.getCognitiveFrameworkInfo());
  }, []);

  useEffect(() => {
    void loadMemories();
    void loadHistory();
    void loadCloud();
  }, [loadCloud, loadHistory, loadMemories]);

  // The being record only exists once the being is awake, so read the framework
  // when the phase reaches `awake` (the awaken dialog refreshes the store, not
  // this page's local state).
  useEffect(() => {
    if (phase === 'awake') void loadFramework();
  }, [loadFramework, phase]);

  useEffect(() => {
    if (forgetPendingId == null) return undefined;
    const timer = window.setTimeout(() => setForgetPendingId(null), FORGET_CONFIRM_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [forgetPendingId]);

  const trendPoints = useMemo(
    () => history
      .map(point => valenceScore(point?.emotion?.valence))
      .filter((score): score is number => score !== null),
    [history],
  );

  const handleLoadEarlier = useCallback(async () => {
    const last = memories[memories.length - 1];
    if (loadingEarlier || typeof last?.timestamp !== 'number') return;
    setLoadingEarlier(true);
    try {
      const result = await trinityAPI.memoryTimeline({
        limit: TIMELINE_LIMIT,
        before_ts: last.timestamp,
        full: true,
      });
      const earlier = normalizeMemoryItems(result);
      if (earlier.length === 0) {
        setTimelineExhausted(true);
      } else {
        setMemories(prev => {
          const seen = new Set(prev.map(m => m.id));
          return [...prev, ...earlier.filter(m => m.id == null || !seen.has(m.id))];
        });
        if (earlier.length < TIMELINE_LIMIT) setTimelineExhausted(true);
      }
    } catch {
      setTimelineExhausted(true);
    } finally {
      setLoadingEarlier(false);
    }
  }, [loadingEarlier, memories]);

  const handleSearch = useCallback(async (event?: React.FormEvent) => {
    event?.preventDefault();
    const query = searchQuery.trim();
    if (!query) {
      setSearchResults(null);
      return;
    }
    setSearching(true);
    try {
      const result = await trinityAPI.recallMemory({ query, limit: 20 });
      const items = Array.isArray(result?.results) ? (result.results as TrinityMemoryEntry[]) : [];
      setSearchResults(items);
    } catch {
      setSearchResults([]);
    } finally {
      setSearching(false);
    }
  }, [searchQuery]);

  const handleAddMemory = useCallback(async () => {
    const content = addContent.trim();
    if (!content || adding) return;
    setAdding(true);
    try {
      const project = addProject.trim();
      await trinityAPI.memorize({
        content,
        kind: 'note',
        ...(project ? { project } : {}),
      });
      setAddContent('');
      setAddProject('');
      setAddOpen(false);
      await loadMemories();
    } catch {
      /* daemon may be offline; form stays filled for retry */
    } finally {
      setAdding(false);
    }
  }, [addContent, addProject, adding, loadMemories]);

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

  const runCloudAction = useCallback(async (key: string, action: () => Promise<unknown>) => {
    setCloudBusy(key);
    setCloudMessage(null);
    try {
      const result = await action() as { hint?: string } | null;
      setCloudMessage(typeof result?.hint === 'string' ? result.hint : null);
      await loadCloud();
    } catch (error) {
      setCloudMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setCloudBusy(null);
    }
  }, [loadCloud]);

  const handleCreateIdentity = useCallback(async () => {
    if (creatingIdentity) return;
    const name = identityName.trim();
    const userName = beingOwner?.trim();
    if (!name || !userName) {
      setCreateIdentityError(t('trinity.scene.createIdentityMissingIdentity'));
      return;
    }

    const confirmed = await confirmDanger(
      t('trinity.scene.createIdentityConfirmTitle'),
      t('trinity.scene.createIdentityConfirmMessage'),
      {
        confirmText: t('trinity.scene.createIdentity'),
        cancelText: t('actions.cancel'),
      },
    );
    if (!confirmed) return;

    setCreatingIdentity(true);
    setCreateIdentityError(null);
    try {
      const workspace = await ensureCognitiveBeingAssistant();
      // `write_file_content` resolves `filePath` as-is, so relative names land
      // in the process CWD. Join the workspace root like personaDocFullPath.
      const base = workspace.rootPath.replace(/[\\/]+$/, '');
      const files = buildCognitiveIdentityFiles({ name, userName, persona });
      for (const [fileName, content] of Object.entries(files)) {
        await workspaceAPI.writeFileContent(base, `${base}/${fileName}`, content);
      }
      await removeBootstrapFile(workspace.rootPath);
      await setPrimaryAssistantWorkspace(workspace.id);

      // The reset already produced a session whose transcript was written under
      // the generic persona, so the recovered identity starts a fresh one.
      const sessionId = await flowChatManager.createChatSession(
        flowChatSessionConfigForWorkspace(workspace),
        ASSISTANT_SESSION_MODE,
      );
      await openMainSession(sessionId, { workspaceId: workspace.id });
    } catch (error) {
      setCreateIdentityError(error instanceof Error ? error.message : String(error));
    } finally {
      setCreatingIdentity(false);
    }
  }, [
    beingOwner,
    creatingIdentity,
    ensureCognitiveBeingAssistant,
    identityName,
    persona,
    setPrimaryAssistantWorkspace,
    t,
  ]);

  const shownMemories = searchResults ?? memories;
  const lastTimelineTs = memories.length > 0 ? memories[memories.length - 1]?.timestamp : undefined;
  const canLoadEarlier = searchResults == null
    && !timelineExhausted
    && memories.length > 0
    && typeof lastTimelineTs === 'number';

  return (
    <div className="openbitfun-trinity-scene" data-openbitfun-scene="trinity" data-openbitfun-part="root">
      <ScrollArea className="openbitfun-trinity-scene__scroll" data-openbitfun-scene="trinity" data-openbitfun-part="scroll">
        <div className="openbitfun-trinity-scene__content" data-openbitfun-scene="trinity" data-openbitfun-part="content">
          <header className="openbitfun-trinity-scene__header">
            <div className="openbitfun-trinity-scene__title-row">
              <Icon glyph={Brain} size="lg" />
              <h1>{identityName}</h1>
              {persona && (
                <span className="openbitfun-trinity-scene__persona-badge">
                  {personaLabel(persona, t)}
                </span>
              )}
              <span
                className="openbitfun-trinity-scene__daemon-badge"
                data-openbitfun-state={phase}
              >
                {t(`trinity.phase.${phase}`)}
              </span>
              {framework && (
                <span
                  className="openbitfun-trinity-scene__framework-chip"
                  title={framework.id}
                  data-openbitfun-part="cognitive-framework"
                >
                  {t('trinity.framework.chip', { count: framework.tools.length })}
                </span>
              )}
              <Button
                variant="outline"
                size="sm"
                leadingIcon={<Icon name="refresh" size="sm" />}
                onClick={() => { void loadMemories(); void loadHistory(); void loadCloud(); void loadFramework(); }}
                data-testid="trinity-scene-refresh"
              >
                {t('trinity.scene.refresh')}
              </Button>
            </div>
            <p className="openbitfun-trinity-scene__subtitle">{t('trinity.scene.subtitle')}</p>
          </header>

          {phase === 'offline' && (
            <Card appearance="raised" padding="md" gap="sm" className="openbitfun-trinity-scene__offline">
              <CardBody>
                <p>{t('trinity.status.unavailable')}</p>
              </CardBody>
            </Card>
          )}

          {phase === 'dormant' && (
            <Card appearance="raised" padding="md" gap="sm" className="openbitfun-trinity-scene__awaken">
              <CardHeader
                contentAlign="center"
                title={<h2>{t('trinity.scene.awakenTitle')}</h2>}
              />
              <CardBody>
                <p>{t('trinity.scene.awakenDescription')}</p>
                <Button
                  variant="primary"
                  leadingIcon={<Icon name="spark" size="sm" />}
                  onClick={() => setAwakenDialogOpen(true)}
                  data-testid="trinity-scene-awaken"
                >
                  {t('trinity.scene.awaken')}
                </Button>
              </CardBody>
            </Card>
          )}

          {shouldCreateIdentity && (
            <Card appearance="raised" padding="md" gap="sm" className="openbitfun-trinity-scene__identity">
              <CardHeader
                contentAlign="center"
                title={<h2>{t('trinity.scene.createIdentityTitle')}</h2>}
              />
              <CardBody>
                <p className="openbitfun-trinity-scene__identity-description">
                  {t('trinity.scene.createIdentityDescription')}
                </p>
                <Button
                  variant="primary"
                  leadingIcon={<Icon name="spark" size="sm" />}
                  disabled={creatingIdentity}
                  onClick={() => { void handleCreateIdentity(); }}
                  data-testid="trinity-create-identity"
                >
                  {creatingIdentity ? t('trinity.scene.creatingIdentity') : t('trinity.scene.createIdentity')}
                </Button>
                {createIdentityError && (
                  <p
                    className="openbitfun-trinity-scene__identity-error"
                    role="alert"
                    data-testid="trinity-create-identity-error"
                  >
                    {t('trinity.scene.createIdentityFailed', { message: createIdentityError })}
                  </p>
                )}
              </CardBody>
            </Card>
          )}

          {phase === 'awake' && psi && (
            <>
              {/* Cloud memory leads the console: it is the action surface
                  (sign up / log in / key ceremony / sync / backup / restore),
                  the rest of the column reports live state. */}
              <CloudCard
                cloudStatus={cloudStatus}
                busy={cloudBusy}
                message={cloudMessage}
                onAction={runCloudAction}
                onAfterRestore={loadMemories}
              />

              <Card appearance="raised" padding="md" gap="sm" className="openbitfun-trinity-scene__state">
                <CardHeader
                  contentAlign="center"
                  title={<h2>{t('trinity.being.state')}</h2>}
                />
                <CardBody>
                  {/* One row, two flag groups: being-state left, engine-state right. */}
                  <div className="openbitfun-trinity-scene__hero">
                    <span className="openbitfun-trinity-scene__hero-group">
                      <span
                        className="openbitfun-trinity-scene__hero-dot"
                        data-openbitfun-emotion={emotionLabelKey(emotion)}
                        aria-hidden="true"
                      />
                      <span className="openbitfun-trinity-scene__hero-emotion">
                        {t(`trinity.emotion.${emotionLabelKey(emotion)}`)}
                      </span>
                      <span className="openbitfun-trinity-scene__hero-focus">
                        {t(`trinity.focus.${focusLabelKey(focus)}`)}
                      </span>
                    </span>
                    <span className="openbitfun-trinity-scene__hero-group openbitfun-trinity-scene__hero-group--end">
                      {typeof confidence === 'number' && (
                        <span className="openbitfun-trinity-scene__hero-meta">
                          {t('trinity.status.confidence')} {formatPercent(confidence)}
                        </span>
                      )}
                      {typeof cycleCount === 'number' && (
                        <span className="openbitfun-trinity-scene__hero-meta">#{cycleCount}</span>
                      )}
                    </span>
                  </div>
                  {identity?.birthday && (
                    <div className="openbitfun-trinity-scene__profile">
                      <span>{t('trinity.profile.birthday')}: {identity.birthday}</span>
                    </div>
                  )}
                  <div className="openbitfun-trinity-scene__needs">
                    <h3>{t('trinity.status.needs')}</h3>
                    {NEED_KEYS.map(key => {
                      const value = needs[key];
                      if (typeof value !== 'number') return null;
                      return (
                        <div className="openbitfun-trinity-scene__need" key={key}>
                          <span className="openbitfun-trinity-scene__need-label">
                            {t(`trinity.needs.${key}`)}
                          </span>
                          <span className="openbitfun-trinity-scene__need-track">
                            <span
                              className="openbitfun-trinity-scene__need-fill"
                              style={{ width: `${value * 100}%` }}
                              data-openbitfun-need={key}
                              data-openbitfun-need-state={needState(value)}
                            />
                          </span>
                          <span className="openbitfun-trinity-scene__need-value">
                            {formatPercent(value)}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                  {trendPoints.length > 1 && (
                    <div className="openbitfun-trinity-scene__trend">
                      <h3>{t('trinity.scene.trend')}</h3>
                      <TrendSparkline points={trendPoints} />
                    </div>
                  )}
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

              <Card appearance="raised" padding="md" gap="sm" className="openbitfun-trinity-scene__timeline">
                <CardHeader
                  contentAlign="center"
                  title={<h2>{t('trinity.scene.memoryTimeline')}</h2>}
                />
                <CardBody>
                  <div className="openbitfun-trinity-scene__timeline-toolbar">
                    <form className="openbitfun-trinity-scene__search" onSubmit={handleSearch}>
                      <input
                        className="openbitfun-config-input"
                        type="search"
                        value={searchQuery}
                        placeholder={t('trinity.memory.searchPlaceholder')}
                        onChange={(event) => setSearchQuery(event.target.value)}
                        data-testid="trinity-memory-search-input"
                      />
                      <Button
                        variant="outline"
                        size="sm"
                        type="submit"
                        leadingIcon={<Icon name="search" size="sm" />}
                        disabled={searching}
                        data-testid="trinity-memory-search"
                      >
                        {t('trinity.memory.search')}
                      </Button>
                      {searchResults != null && (
                        <Button
                          variant="text"
                          size="sm"
                          onClick={() => { setSearchQuery(''); setSearchResults(null); }}
                          data-testid="trinity-memory-search-clear"
                        >
                          {t('trinity.memory.backToTimeline')}
                        </Button>
                      )}
                    </form>
                    <Button
                      variant="outline"
                      size="sm"
                      leadingIcon={<Icon name="plus" size="sm" />}
                      onClick={() => setAddOpen(value => !value)}
                      data-testid="trinity-memory-add-toggle"
                    >
                      {t('trinity.memory.addMemory')}
                    </Button>
                  </div>

                  {addOpen && (
                    <form
                      className="openbitfun-trinity-scene__add-form"
                      onSubmit={(event) => { event.preventDefault(); void handleAddMemory(); }}
                    >
                      <textarea
                        className="openbitfun-config-input"
                        rows={3}
                        value={addContent}
                        placeholder={t('trinity.memory.addContentPlaceholder')}
                        onChange={(event) => setAddContent(event.target.value)}
                        data-testid="trinity-memory-add-content"
                      />
                      <div className="openbitfun-trinity-scene__add-row">
                        <input
                          className="openbitfun-config-input"
                          type="text"
                          value={addProject}
                          placeholder={t('trinity.memory.addProject')}
                          onChange={(event) => setAddProject(event.target.value)}
                          data-testid="trinity-memory-add-project"
                        />
                        <Button
                          variant="primary"
                          size="sm"
                          type="submit"
                          disabled={adding || !addContent.trim()}
                          data-testid="trinity-memory-add-submit"
                        >
                          {t('trinity.memory.addSubmit')}
                        </Button>
                      </div>
                    </form>
                  )}

                  {searchResults != null && (
                    <p className="openbitfun-trinity-scene__section-label">
                      {t('trinity.memory.searchResults')}
                    </p>
                  )}

                  {memoriesLoading ? (
                    <Spinner size="md" />
                  ) : shownMemories.length === 0 ? (
                    <p className="openbitfun-trinity-scene__empty">
                      {searchResults != null
                        ? t('trinity.memory.noSearchResults')
                        : t('trinity.scene.noMemories')}
                    </p>
                  ) : (
                    <ul className="openbitfun-trinity-scene__memory-list">
                      {shownMemories.map((entry, index) => {
                        const time = formatMemoryTime(entry);
                        const forgetArmed = entry.id != null && forgetPendingId === entry.id;
                        return (
                          <li className="openbitfun-trinity-scene__memory-item" key={entry.id ?? `r-${index}`}>
                            {entry.kind && (
                              <span className="openbitfun-trinity-scene__memory-kind">{entry.kind}</span>
                            )}
                            {entry.layer && (
                              <span
                                className="openbitfun-trinity-scene__memory-layer"
                                data-openbitfun-layer={entry.layer}
                              >
                                {entry.layer}
                              </span>
                            )}
                            <OverflowText className="openbitfun-trinity-scene__memory-content">
                              {entry.content}
                            </OverflowText>
                            {typeof entry.strength === 'number' && (
                              <span className="openbitfun-trinity-scene__memory-strength">
                                {formatPercent(entry.strength)}
                              </span>
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
                                  <Icon name="delete" size="sm" />
                                </button>
                              </span>
                            )}
                          </li>
                        );
                      })}
                    </ul>
                  )}

                  {canLoadEarlier && (
                    <div className="openbitfun-trinity-scene__timeline-more">
                      <Button
                        variant="text"
                        size="sm"
                        disabled={loadingEarlier}
                        onClick={() => { void handleLoadEarlier(); }}
                        data-testid="trinity-memory-load-earlier"
                      >
                        {t('trinity.memory.loadEarlier')}
                      </Button>
                    </div>
                  )}
                  {timelineExhausted && memories.length > 0 && searchResults == null && (
                    <p className="openbitfun-trinity-scene__timeline-end">
                      {t('trinity.memory.noMore')}
                    </p>
                  )}
                </CardBody>
              </Card>
            </>
          )}
        </div>
      </ScrollArea>

      <TrinityAwakenDialog
        open={awakenDialogOpen}
        onClose={() => setAwakenDialogOpen(false)}
        workspacePath={awakenWorkspacePath}
      />
    </div>
  );
};


export default TrinityScene;
