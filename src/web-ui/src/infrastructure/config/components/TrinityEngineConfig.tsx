/**
 * Trinity cognitive engine settings page.
 *
 * Reads/writes the trinityd LLM configuration through the desktop host and
 * tests the connection. The connection fields (provider presets + test) are
 * shared with the onboarding wizard (product plan P0-2); the daemon owns the
 * toml, this page only forwards.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { Button, Icon, Spinner } from '@openbitfun/ui';
import { useTranslation } from 'react-i18next';
import { useNotification } from '@/shared/notification-system';
import { trinityAPI } from '@/infrastructure/api';
import TrinityLlmFields, { type TrinityLlmFieldsValue } from './TrinityLlmFields';
import {
  ConfigPageContent,
  ConfigPageHeader,
  ConfigPageLayout,
  ConfigPageSection,
} from './common';

const TrinityEngineConfig: React.FC = () => {
  const { t } = useTranslation('settings');
  const { success, error: notifyError } = useNotification();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [llm, setLlm] = useState<TrinityLlmFieldsValue>({ apiUrl: '', model: '', apiKey: '' });

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await trinityAPI.llmGetConfig();
      setLlm({
        apiUrl: result?.api_url ?? '',
        model: result?.model ?? '',
        apiKey: result?.api_key ?? '',
      });
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
      await trinityAPI.llmSetConfig({ api_url: llm.apiUrl, model: llm.model, api_key: llm.apiKey });
      success(t('trinity.engine.saved'));
      await load();
    } catch (err) {
      notifyError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }, [llm, load, notifyError, success, t]);

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
            <TrinityLlmFields
              value={llm}
              onChange={patch => setLlm(previous => ({ ...previous, ...patch }))}
            />
            <div className="openbitfun-config-actions">
              <Button
                variant="primary"
                size="sm"
                leadingIcon={<Icon name="check-line" size="sm" />}
                onClick={() => { void handleSave(); }}
                disabled={saving}
                data-testid="trinity-engine-save"
              >
                {saving ? t('trinity.engine.saving') : t('trinity.engine.save')}
              </Button>
            </div>
          </ConfigPageSection>
        )}
      </ConfigPageContent>
    </ConfigPageLayout>
  );
};

export default TrinityEngineConfig;
