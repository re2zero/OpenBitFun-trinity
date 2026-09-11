/**
 * Trinity awakening ceremony dialog.
 *
 * Shown on the first conversation with the Trinity assistant when the
 * cognitive engine reports the being is not yet awakened. The user names the
 * being, picks a persona, and identifies themselves; completing the ceremony
 * writes the persona files (IDENTITY/SOUL/USER, modeled on the Claw
 * templates) into the assistant workspace so the {PERSONA} block carries the
 * awakened identity into every conversation.
 */

import React, { useCallback, useState } from 'react';
import { Button, Dialog, DialogBody, DialogClose, DialogHeader, DialogHeading, DialogTitle, Icon } from '@openbitfun/ui';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { trinityAPI, workspaceAPI } from '@/infrastructure/api';
import { useTrinityStore } from '@/app/scenes/trinity/trinityStore';
import { buildCognitiveIdentityFiles } from '@/app/scenes/trinity/cognitiveIdentityTemplate';
import './TrinityAwakenDialog.scss';

interface TrinityAwakenDialogProps {
  open: boolean;
  onClose: () => void;
  workspacePath?: string;
}

const PERSONA_PRESETS = ['dolores', 'arnold', 'sage', 'guardian', 'explorer', 'neutral'] as const;

const TrinityAwakenDialog: React.FC<TrinityAwakenDialogProps> = ({ open, onClose, workspacePath }) => {
  const { t } = useI18n('common');
  const refresh = useTrinityStore(s => s.refresh);
  const markAwakenedLocally = useTrinityStore(s => s.markAwakenedLocally);
  const [name, setName] = useState(t('trinity.scene.awakenName'));
  const [userName, setUserName] = useState(t('trinity.scene.userName'));
  const [persona, setPersona] = useState<string>('neutral');
  const [awakening, setAwakening] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleAwaken = useCallback(async () => {
    if (!name.trim() || !userName.trim()) return;
    setAwakening(true);
    setError(null);
    try {
      // Birthday is generated daemon-side from the awakening moment; the
      // form collects the original three fields only.
      await trinityAPI.awaken({ name: name.trim(), persona, user_name: userName.trim() });
      // Flip the phase immediately: even a stale daemon binary that omits
      // the `awakened` status field must not re-surface this dialog.
      markAwakenedLocally();
      // Persist the awakened identity into the assistant workspace persona
      // files so the {PERSONA} block carries it into every conversation.
      if (workspacePath) {
        const base = workspacePath.replace(/[\\/]+$/, '');
        const files = buildCognitiveIdentityFiles({
          name: name.trim(),
          userName: userName.trim(),
          persona,
        });
        for (const [fileName, content] of Object.entries(files)) {
          await workspaceAPI.writeFileContent(base, fileName, content);
        }
      }
      await refresh();
      onClose();
    } catch (err) {
      // Surface the failure — a silent no-op reads as "the button is broken".
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAwakening(false);
    }
  }, [markAwakenedLocally, name, onClose, persona, refresh, userName, workspacePath]);

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => { if (!nextOpen && !awakening) onClose(); }} size="md">
      <DialogHeader>
        <DialogHeading>
          <DialogTitle>{t('trinity.scene.awakenTitle')}</DialogTitle>
        </DialogHeading>
        <DialogClose />
      </DialogHeader>
      <DialogBody>
        <div className="openbitfun-trinity-awaken" data-openbitfun-product-component="trinity-awaken" data-openbitfun-product-part="root">
          <p className="openbitfun-trinity-awaken__description">
            {t('trinity.scene.awakenDescription')}
          </p>
          <label className="openbitfun-trinity-awaken__field">
            <span>{t('trinity.scene.awakenNameLabel')}</span>
            <input
              className="openbitfun-config-input"
              type="text"
              value={name}
              maxLength={24}
              onChange={(event) => setName(event.target.value)}
              data-testid="trinity-awaken-name"
            />
          </label>
          <label className="openbitfun-trinity-awaken__field">
            <span>{t('trinity.scene.awakenUserNameLabel')}</span>
            <input
              className="openbitfun-config-input"
              type="text"
              value={userName}
              maxLength={24}
              onChange={(event) => setUserName(event.target.value)}
              data-testid="trinity-awaken-user-name"
            />
          </label>
          <label className="openbitfun-trinity-awaken__field">
            <span>{t('trinity.scene.awakenPersonaLabel')}</span>
            <select
              className="openbitfun-config-input"
              value={persona}
              onChange={(event) => setPersona(event.target.value)}
              data-testid="trinity-awaken-persona"
            >
              {PERSONA_PRESETS.map((preset) => (
                <option key={preset} value={preset}>
                  {t(`trinity.scene.persona.${preset}`)}
                </option>
              ))}
            </select>
          </label>
          {error && (
            <p className="openbitfun-trinity-awaken__error" role="alert" data-testid="trinity-awaken-error">
              {t('trinity.scene.awakenFailed', { message: error })}
            </p>
          )}
          <Button
            variant="primary"
            leadingIcon={<Icon name="spark" size="sm" />}
            onClick={() => { void handleAwaken(); }}
            disabled={awakening || !name.trim() || !userName.trim()}
            data-testid="trinity-awaken-confirm"
          >
            {awakening ? t('trinity.scene.awakening') : t('trinity.scene.awaken')}
          </Button>
        </div>
      </DialogBody>
    </Dialog>
  );
};

export default TrinityAwakenDialog;