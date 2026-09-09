/**
 * Trinity cognitive engine settings page.
 *
 * Reads/writes the trinityd LLM configuration through the desktop host and
 * tests the connection. The daemon owns the toml; this page only forwards.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { Button, Icon, Spinner } from '@openbitfun/ui';
import { Check } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNotification } from '@/shared/notification-system';
import { trinityAPI } from '@/infrastructure/api';
import {
  ConfigPageContent,
  ConfigPageHeader,
  ConfigPageLayout,
  ConfigPageRow,
  ConfigPageSection,
} from './common';

const TrinityEngineConfig: React.FC = () => {
  const { t } = useTranslation('settings');
  const { success, error: notifyError } = useNotification();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [apiUrl, setApiUrl] = useState('');
  const [model, setModel] = useState('');
  const [apiKey, setApiKey] = useState('');

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await trinityAPI.llmGetConfig();
      setApiUrl(result?.api_url ?? '');
      setModel(result?.model ?? '');
      setApiKey(result?.api_key ?? '');
    } catch {
      // daemon offline; leave fields empty
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await trinityAPI.llmSetConfig({ api_url: apiUrl, model, api_key: apiKey });
      success(t('trinity.engine.saved'));
      await load();
    } catch (err) {
      notifyError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }, [apiKey, apiUrl, load, model, notifyError, success, t]);

  const handleTest = useCallback(async () => {
    setTesting(true);
    try {
      const result = await trinityAPI.llmTestConnection({ api_url: apiUrl, model, api_key: apiKey });
      if (result?.ok) {
        success(t('trinity.engine.connectionOk'));
      } else {
        notifyError(result?.error ?? t('trinity.engine.connectionFailed'));
      }
    } catch (err) {
      notifyError(err instanceof Error ? err.message : String(err));
    } finally {
      setTesting(false);
    }
  }, [apiKey, apiUrl, model, notifyError, success, t]);

  return (
    <ConfigPageLayout>
      <ConfigPageHeader
        title={t('navigation.pages.trinity.label')}
        subtitle={t('navigation.pages.trinity.description')}
      />
      <ConfigPageContent>
        {loading ? (
          <Spinner size="md" />
        ) : (
          <ConfigPageSection title={t('trinity.engine.llmSection')}>
            <ConfigPageRow label={t('trinity.engine.apiUrl')}>
              <input
                className="openbitfun-config-input"
                type="text"
                value={apiUrl}
                onChange={(event) => setApiUrl(event.target.value)}
                placeholder="https://api.openai.com/v1"
                data-testid="trinity-engine-api-url"
              />
            </ConfigPageRow>
            <ConfigPageRow label={t('trinity.engine.model')}>
              <input
                className="openbitfun-config-input"
                type="text"
                value={model}
                onChange={(event) => setModel(event.target.value)}
                placeholder="gpt-4o"
                data-testid="trinity-engine-model"
              />
            </ConfigPageRow>
            <ConfigPageRow label={t('trinity.engine.apiKey')}>
              <input
                className="openbitfun-config-input"
                type="password"
                value={apiKey}
                onChange={(event) => setApiKey(event.target.value)}
                placeholder="sk-..."
                data-testid="trinity-engine-api-key"
              />
            </ConfigPageRow>
            <div className="openbitfun-config-actions">
              <Button
                variant="primary"
                size="sm"
                leadingIcon={<Icon glyph={Check} size="sm" />}
                onClick={() => { void handleSave(); }}
                disabled={saving}
                data-testid="trinity-engine-save"
              >
                {saving ? t('trinity.engine.saving') : t('trinity.engine.save')}
              </Button>
              <Button
                variant="outline"
                size="sm"
                leadingIcon={<Icon name="link" size="sm" />}
                onClick={() => { void handleTest(); }}
                disabled={testing}
                data-testid="trinity-engine-test"
              >
                {testing ? t('trinity.engine.testing') : t('trinity.engine.testConnection')}
              </Button>
            </div>
          </ConfigPageSection>
        )}
      </ConfigPageContent>
    </ConfigPageLayout>
  );
};

export default TrinityEngineConfig;