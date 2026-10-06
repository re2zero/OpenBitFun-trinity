/**
 * Shared cognitive-engine LLM connection fields.
 *
 * Provider presets + endpoint/model/key inputs + an inline connection test.
 * Used by the settings page (TrinityEngineConfig) and the onboarding wizard's
 * LLM step — one component so the two surfaces stay in sync (product plan
 * P0-2). The daemon owns the toml; callers persist via trinityAPI.llmSetConfig.
 */

import React, { useCallback, useState } from 'react';
import { Button, Icon } from '@openbitfun/ui';
import { useTranslation } from 'react-i18next';
import { trinityAPI } from '@/infrastructure/api';
import './TrinityLlmFields.scss';

export interface TrinityLlmFieldsValue {
  apiUrl: string;
  model: string;
  apiKey: string;
}

interface TrinityLlmFieldsProps {
  value: TrinityLlmFieldsValue;
  onChange: (patch: Partial<TrinityLlmFieldsValue>) => void;
}

/** Presets fill url+model; the api key always comes from the user. */
const PROVIDER_PRESETS = [
  { id: 'deepseek', apiUrl: 'https://api.deepseek.com/v1', model: 'deepseek-chat' },
  { id: 'zhipu', apiUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash' },
  { id: 'openai', apiUrl: 'https://api.openai.com/v1', model: 'gpt-4o' },
  { id: 'ollama', apiUrl: 'http://localhost:11434/v1', model: 'qwen3:8b' },
  { id: 'custom', apiUrl: '', model: '' },
] as const;

type ProviderPresetId = (typeof PROVIDER_PRESETS)[number]['id'];

const detectPreset = (value: TrinityLlmFieldsValue): ProviderPresetId => (
  PROVIDER_PRESETS.find(preset => preset.id !== 'custom'
    && preset.apiUrl === value.apiUrl
    && preset.model === value.model)?.id ?? 'custom'
);

const TrinityLlmFields: React.FC<TrinityLlmFieldsProps> = ({ value, onChange }) => {
  const { t } = useTranslation('settings');
  const [preset, setPreset] = useState<ProviderPresetId>(() => detectPreset(value));
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; detail: string } | null>(null);

  const applyPreset = useCallback((id: ProviderPresetId) => {
    setPreset(id);
    const target = PROVIDER_PRESETS.find(p => p.id === id);
    if (!target) return;
    onChange({ apiUrl: target.apiUrl, model: target.model });
    setTestResult(null);
  }, [onChange]);

  const handleTest = useCallback(async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await trinityAPI.llmTestConnection({
        api_url: value.apiUrl,
        model: value.model,
        api_key: value.apiKey,
      });
      if (result?.ok) {
        setTestResult({ ok: true, detail: t('trinity.engine.connectionOk') });
      } else {
        setTestResult({ ok: false, detail: result?.error ?? t('trinity.engine.connectionFailed') });
      }
    } catch (err) {
      setTestResult({ ok: false, detail: err instanceof Error ? err.message : String(err) });
    } finally {
      setTesting(false);
    }
  }, [t, value.apiUrl, value.apiKey, value.model]);

  // The api key is optional: the daemon falls back to the saved key when the
  // field is empty and reports honestly when none exists.
  const canTest = !testing && value.apiUrl.trim() !== '' && value.model.trim() !== '';

  return (
    <div
      className="openbitfun-trinity-llm-fields"
      data-openbitfun-component="trinity-llm-fields"
      data-openbitfun-part="root"
      data-testid="trinity-llm-fields"
    >
      <div className="openbitfun-trinity-llm-fields__presets" role="radiogroup" aria-label={t('trinity.engine.providerLabel')}>
        {PROVIDER_PRESETS.map(entry => (
          <button
            key={entry.id}
            type="button"
            role="radio"
            aria-checked={preset === entry.id}
            className={[
              'openbitfun-trinity-llm-fields__preset',
              preset === entry.id ? 'is-active' : '',
            ].filter(Boolean).join(' ')}
            onClick={() => applyPreset(entry.id)}
            data-testid={`trinity-llm-preset-${entry.id}`}
          >
            {t(`trinity.engine.provider.${entry.id}`)}
          </button>
        ))}
      </div>
      <label className="openbitfun-trinity-llm-fields__field">
        <span>{t('trinity.engine.apiUrl')}</span>
        <input
          className="openbitfun-config-input"
          type="text"
          value={value.apiUrl}
          onChange={event => onChange({ apiUrl: event.target.value })}
          placeholder="https://api.openai.com/v1"
          data-testid="trinity-llm-api-url"
        />
      </label>
      <label className="openbitfun-trinity-llm-fields__field">
        <span>{t('trinity.engine.model')}</span>
        <input
          className="openbitfun-config-input"
          type="text"
          value={value.model}
          onChange={event => onChange({ model: event.target.value })}
          placeholder="gpt-4o"
          data-testid="trinity-llm-model"
        />
      </label>
      <label className="openbitfun-trinity-llm-fields__field">
        <span>{t('trinity.engine.apiKey')}</span>
        <input
          className="openbitfun-config-input"
          type="password"
          value={value.apiKey}
          onChange={event => onChange({ apiKey: event.target.value })}
          placeholder="sk-..."
          data-testid="trinity-llm-api-key"
        />
      </label>
      <div className="openbitfun-trinity-llm-fields__actions">
        <Button
          variant="outline"
          size="sm"
          leadingIcon={<Icon name="link" size="sm" />}
          onClick={() => { void handleTest(); }}
          disabled={!canTest}
          data-testid="trinity-llm-test"
        >
          {testing ? t('trinity.engine.testing') : t('trinity.engine.testConnection')}
        </Button>
        {testResult && (
          <span
            className={[
              'openbitfun-trinity-llm-fields__test-result',
              testResult.ok ? 'is-ok' : 'is-fail',
            ].filter(Boolean).join(' ')}
            role="status"
            data-testid="trinity-llm-test-result"
          >
            {testResult.detail}
          </span>
        )}
      </div>
    </div>
  );
};

export default TrinityLlmFields;
