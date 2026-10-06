/**
 * Full-screen awakening ceremony (product plan P0-1).
 *
 * First launch: the factory being ships un-awakened, so the dormant phase
 * means "the wizard has never run". Surfaces wherever the user is — no
 * workspace precondition — and walks naming → persona → owner → LLM config,
 * then performs the awakening: engine ceremony RPC, default workspace
 * auto-creation (P0-3), identity files, and a completion view that lands the
 * user in her scene ready to talk.
 */

import React, { useCallback, useMemo, useState } from 'react';
import { Button, Icon } from '@openbitfun/ui';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { trinityAPI, workspaceAPI } from '@/infrastructure/api';
import { workspaceManager } from '@/infrastructure/services/business/workspaceManager';
import { useSceneStore } from '@/app/stores/sceneStore';
import { useTrinityStore } from '@/app/scenes/trinity/trinityStore';
import { buildCognitiveIdentityFiles } from '@/app/scenes/trinity/cognitiveIdentityTemplate';
import TrinityLlmFields, { type TrinityLlmFieldsValue } from '@/infrastructure/config/components/TrinityLlmFields';
import './OnboardingWizard.scss';

const PERSONA_PRESETS = ['dolores', 'arnold', 'sage', 'guardian', 'explorer', 'neutral'] as const;

type PersonaPreset = (typeof PERSONA_PRESETS)[number];

type OnboardingStep = 'identity' | 'persona' | 'owner' | 'llm' | 'complete';

const STEP_ORDER: Exclude<OnboardingStep, 'complete'>[] = ['identity', 'persona', 'owner', 'llm'];

interface OnboardingWizardProps {
  open: boolean;
  onClose: () => void;
}

const OnboardingWizard: React.FC<OnboardingWizardProps> = ({ open, onClose }) => {
  const { t } = useI18n('common');
  const refresh = useTrinityStore(s => s.refresh);
  const markAwakenedLocally = useTrinityStore(s => s.markAwakenedLocally);
  const [step, setStep] = useState<OnboardingStep>('identity');
  const [name, setName] = useState(t('trinity.scene.awakenName'));
  const [userName, setUserName] = useState(t('trinity.scene.userName'));
  const [persona, setPersona] = useState<PersonaPreset>('neutral');
  const [llm, setLlm] = useState<TrinityLlmFieldsValue>({ apiUrl: '', model: '', apiKey: '' });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const stepIndex = useMemo(() => STEP_ORDER.indexOf(step as Exclude<OnboardingStep, 'complete'>), [step]);

  const handleLlmChange = useCallback((patch: Partial<TrinityLlmFieldsValue>) => {
    setLlm(previous => ({ ...previous, ...patch }));
  }, []);

  const goBack = useCallback(() => {
    if (stepIndex > 0) setStep(STEP_ORDER[stepIndex - 1]);
  }, [stepIndex]);

  const goNext = useCallback(() => {
    if (stepIndex < STEP_ORDER.length - 1) setStep(STEP_ORDER[stepIndex + 1]);
  }, [stepIndex]);

  /** The awakening itself: LLM config (optional) → ceremony → workspace → identity files. */
  const beginAwaken = useCallback(async () => {
    if (!name.trim() || !userName.trim()) return;
    setBusy(true);
    setError(null);
    try {
      // LLM config is optional — leaving it empty defers to the settings
      // page; the engine's simulated backend keeps her usable regardless.
      if (llm.apiUrl.trim() && llm.model.trim()) {
        await trinityAPI.llmSetConfig({ api_url: llm.apiUrl.trim(), model: llm.model.trim(), api_key: llm.apiKey });
      }
      // Birthday is generated daemon-side from the awakening moment; the
      // wizard collects the original three fields only.
      await trinityAPI.awaken({ name: name.trim(), persona, user_name: userName.trim() });
      // Flip the phase immediately: even a stale daemon binary that omits
      // the `awakened` status field must not re-surface this wizard.
      markAwakenedLocally();
      // Default workspace auto-creation (P0-3): a first launch has no
      // workspace, so the ceremony creates the cognitive-being one and makes
      // it current — no disabled states, ready to talk when the wizard closes.
      const workspace = await workspaceManager.ensureCognitiveBeingAssistant();
      await workspaceManager.switchWorkspace(workspace);
      // Persist the awakened identity into the assistant workspace persona
      // files so the {PERSONA} block carries it into every conversation.
      if (workspace?.rootPath) {
        const base = workspace.rootPath.replace(/[\\/]+$/, '');
        const files = buildCognitiveIdentityFiles({
          name: name.trim(),
          userName: userName.trim(),
          persona,
        });
        for (const [fileName, content] of Object.entries(files)) {
          await workspaceAPI.writeFileContent(base, `${base}/${fileName}`, content);
        }
      }
      await refresh();
      setStep('complete');
    } catch (err) {
      // Surface the failure — a silent no-op reads as "the button is broken".
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [llm.apiKey, llm.apiUrl, llm.model, markAwakenedLocally, name, persona, refresh, userName]);

  const enterScene = useCallback(() => {
    useSceneStore.getState().openScene('trinity');
    onClose();
  }, [onClose]);

  if (!open) return null;

  return (
    <div
      className="openbitfun-trinity-onboarding"
      data-openbitfun-product-component="trinity-awaken"
      data-openbitfun-product-part="root"
      data-testid="trinity-onboarding"
    >
      <div className="openbitfun-trinity-onboarding__card">
        <header className="openbitfun-trinity-onboarding__header">
          <h1 className="openbitfun-trinity-onboarding__title">{t('trinity.scene.awakenTitle')}</h1>
          {!busy && (
            <button
              type="button"
              className="openbitfun-trinity-onboarding__dismiss"
              onClick={onClose}
              aria-label={t('trinity.onboarding.later')}
              data-testid="trinity-onboarding-dismiss"
            >
              {t('trinity.onboarding.later')}
            </button>
          )}
        </header>

        {step !== 'complete' && (
          <ol className="openbitfun-trinity-onboarding__steps" aria-label={t('trinity.scene.awakenTitle')}>
            {STEP_ORDER.map((entry, index) => (
              <li
                key={entry}
                className={[
                  'openbitfun-trinity-onboarding__step',
                  entry === step ? 'is-current' : '',
                  index < stepIndex ? 'is-done' : '',
                ].filter(Boolean).join(' ')}
                aria-current={entry === step ? 'step' : undefined}
              >
                {t(`trinity.onboarding.steps.${entry}`)}
              </li>
            ))}
          </ol>
        )}

        {step === 'identity' && (
          <section className="openbitfun-trinity-onboarding__body">
            <p className="openbitfun-trinity-onboarding__hint">{t('trinity.onboarding.welcome')}</p>
            <label className="openbitfun-trinity-onboarding__field">
              <span>{t('trinity.scene.awakenNameLabel')}</span>
              <input
                className="openbitfun-config-input"
                type="text"
                value={name}
                maxLength={24}
                onChange={event => setName(event.target.value)}
                autoFocus
                data-testid="trinity-onboarding-name"
              />
            </label>
          </section>
        )}

        {step === 'persona' && (
          <section className="openbitfun-trinity-onboarding__body">
            <p className="openbitfun-trinity-onboarding__hint">{t('trinity.onboarding.personaHint')}</p>
            <div className="openbitfun-trinity-onboarding__personas" role="radiogroup" aria-label={t('trinity.scene.awakenPersonaLabel')}>
              {PERSONA_PRESETS.map(preset => (
                <button
                  key={preset}
                  type="button"
                  role="radio"
                  aria-checked={persona === preset}
                  className={[
                    'openbitfun-trinity-onboarding__persona',
                    persona === preset ? 'is-active' : '',
                  ].filter(Boolean).join(' ')}
                  onClick={() => setPersona(preset)}
                  data-testid={`trinity-onboarding-persona-${preset}`}
                >
                  <span className="openbitfun-trinity-onboarding__persona-name">
                    {t(`trinity.scene.persona.${preset}`)}
                  </span>
                  <span className="openbitfun-trinity-onboarding__persona-desc">
                    {t(`trinity.onboarding.personaDesc.${preset}`)}
                  </span>
                </button>
              ))}
            </div>
          </section>
        )}

        {step === 'owner' && (
          <section className="openbitfun-trinity-onboarding__body">
            <p className="openbitfun-trinity-onboarding__hint">{t('trinity.onboarding.ownerHint')}</p>
            <label className="openbitfun-trinity-onboarding__field">
              <span>{t('trinity.scene.awakenUserNameLabel')}</span>
              <input
                className="openbitfun-config-input"
                type="text"
                value={userName}
                maxLength={24}
                onChange={event => setUserName(event.target.value)}
                data-testid="trinity-onboarding-user-name"
              />
            </label>
          </section>
        )}

        {step === 'llm' && (
          <section className="openbitfun-trinity-onboarding__body">
            <p className="openbitfun-trinity-onboarding__hint">{t('trinity.onboarding.llmHint')}</p>
            <TrinityLlmFields value={llm} onChange={handleLlmChange} />
          </section>
        )}

        {step === 'complete' && (
          <section className="openbitfun-trinity-onboarding__body openbitfun-trinity-onboarding__body--complete">
            <Icon name="spark" size="lg" className="openbitfun-trinity-onboarding__spark" />
            <h2 className="openbitfun-trinity-onboarding__complete-title">
              {t('trinity.onboarding.completeTitle', { name: name.trim() })}
            </h2>
            <p className="openbitfun-trinity-onboarding__complete-message">
              {t('trinity.onboarding.completeMessage')}
            </p>
          </section>
        )}

        {error && (
          <p className="openbitfun-trinity-onboarding__error" role="alert" data-testid="trinity-onboarding-error">
            {t('trinity.scene.awakenFailed', { message: error })}
          </p>
        )}

        <footer className="openbitfun-trinity-onboarding__footer">
          {step !== 'complete' ? (
            <>
              <Button
                variant="text"
                onClick={goBack}
                disabled={busy || stepIndex === 0}
                data-testid="trinity-onboarding-back"
              >
                {t('trinity.onboarding.back')}
              </Button>
              {step === 'llm' ? (
                <Button
                  variant="primary"
                  leadingIcon={<Icon name="spark" size="sm" />}
                  onClick={() => { void beginAwaken(); }}
                  disabled={busy || !name.trim() || !userName.trim()}
                  data-testid="trinity-onboarding-begin"
                >
                  {busy ? t('trinity.scene.awakening') : t('trinity.onboarding.begin')}
                </Button>
              ) : (
                <Button
                  variant="primary"
                  onClick={goNext}
                  disabled={busy || (step === 'identity' && !name.trim())}
                  data-testid="trinity-onboarding-next"
                >
                  {t('trinity.onboarding.next')}
                </Button>
              )}
            </>
          ) : (
            <Button
              variant="primary"
              onClick={enterScene}
              data-testid="trinity-onboarding-enter"
            >
              {t('trinity.onboarding.enter')}
            </Button>
          )}
        </footer>
      </div>
    </div>
  );
};

export default OnboardingWizard;
