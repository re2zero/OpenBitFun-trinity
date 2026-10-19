// @vitest-environment jsdom

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

vi.mock('@/infrastructure/i18n/hooks/useI18n', async () => {
  const { createTestI18nT } = await import('@/test/i18nTestUtils');
  return { useI18n: () => ({ t: createTestI18nT('common') }) };
});

// The LLM step renders the shared fields component, which reads the settings
// namespace directly through react-i18next.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const apiHarness = vi.hoisted(() => ({
  awaken: vi.fn(),
  llmSetConfig: vi.fn(),
  llmTestConnection: vi.fn(),
  writeFileContent: vi.fn(),
}));

vi.mock('@/infrastructure/api', () => ({
  trinityAPI: apiHarness,
  workspaceAPI: { writeFileContent: apiHarness.writeFileContent },
}));

const storeHarness = vi.hoisted(() => ({
  refresh: vi.fn(),
  markAwakenedLocally: vi.fn(),
}));

vi.mock('@/app/scenes/trinity/trinityStore', () => ({
  useTrinityStore: (selector: (state: Record<string, unknown>) => unknown) => selector({
    refresh: storeHarness.refresh,
    markAwakenedLocally: storeHarness.markAwakenedLocally,
  }),
}));

const sceneHarness = vi.hoisted(() => ({ openScene: vi.fn() }));

vi.mock('@/app/stores/sceneStore', () => ({
  useSceneStore: { getState: () => ({ openScene: sceneHarness.openScene }) },
}));

vi.mock('@/infrastructure/services/business/workspaceManager', () => ({
  workspaceManager: {
    ensureCognitiveBeingAssistant: vi.fn(),
    switchWorkspace: vi.fn(),
  },
}));

import OnboardingWizard from './OnboardingWizard';
import { workspaceManager } from '@/infrastructure/services/business/workspaceManager';

const WORKSPACE = { id: 'ws-trinity', rootPath: '/tmp/ws/trinity' };

describe('OnboardingWizard', () => {
  let container: HTMLDivElement;
  let root: Root;
  let onClose: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.mocked(workspaceManager.ensureCognitiveBeingAssistant).mockReset();
    vi.mocked(workspaceManager.switchWorkspace).mockReset();
    vi.mocked(workspaceManager.ensureCognitiveBeingAssistant).mockResolvedValue(WORKSPACE as never);
    vi.mocked(workspaceManager.switchWorkspace).mockResolvedValue(WORKSPACE as never);
    apiHarness.awaken.mockReset().mockResolvedValue({});
    apiHarness.llmSetConfig.mockReset().mockResolvedValue({});
    apiHarness.llmTestConnection.mockReset().mockResolvedValue({ ok: true });
    apiHarness.writeFileContent.mockReset().mockResolvedValue(undefined);
    storeHarness.refresh.mockReset();
    storeHarness.markAwakenedLocally.mockReset();
    sceneHarness.openScene.mockReset();
    onClose = vi.fn();
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  const mount = (open = true) => act(() => {
    root.render(<OnboardingWizard open={open} onClose={onClose} />);
  });

  const byTestId = (testId: string) => {
    const element = container.querySelector(`[data-testid="${testId}"]`);
    if (!element) throw new Error(`missing [data-testid="${testId}"]`);
    return element;
  };

  const click = async (element: Element) => {
    await act(async () => {
      element.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    });
  };

  const typeInto = (element: Element, value: string) => act(() => {
    const input = element as HTMLInputElement;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
    setter?.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });

  it('walks the full ceremony: naming → persona → owner → LLM → completion', async () => {
    await mount();

    // Step 1: naming (defaults pre-filled from the factory locale).
    expect((byTestId('trinity-onboarding-name') as HTMLInputElement).value).toBe('Lingshu');
    await click(byTestId('trinity-onboarding-next'));

    // Step 2: persona cards.
    await click(byTestId('trinity-onboarding-persona-sage'));
    await click(byTestId('trinity-onboarding-next'));

    // Step 3: owner (default pre-filled).
    expect((byTestId('trinity-onboarding-user-name') as HTMLInputElement).value).toBe('Companion');
    await click(byTestId('trinity-onboarding-next'));

    // Step 4: LLM — filled here so the save path is exercised too.
    typeInto(byTestId('trinity-llm-api-url'), 'https://api.deepseek.com/v1');
    typeInto(byTestId('trinity-llm-model'), 'deepseek-chat');
    await click(byTestId('trinity-onboarding-begin'));

    expect(apiHarness.llmSetConfig).toHaveBeenCalledWith({
      api_url: 'https://api.deepseek.com/v1', model: 'deepseek-chat', api_key: '',
    });
    expect(apiHarness.awaken).toHaveBeenCalledWith({
      name: 'Lingshu', persona: 'sage', user_name: 'Companion',
    });
    expect(storeHarness.markAwakenedLocally).toHaveBeenCalled();
    // Default workspace auto-created and made current (P0-3).
    expect(workspaceManager.ensureCognitiveBeingAssistant).toHaveBeenCalled();
    expect(workspaceManager.switchWorkspace).toHaveBeenCalledWith(WORKSPACE);
    // Identity files land in the assistant workspace.
    expect(apiHarness.writeFileContent).toHaveBeenCalledTimes(3);
    expect(String(apiHarness.writeFileContent.mock.calls[0][0])).toBe('/tmp/ws/trinity');

    // Completion view, then landing in her scene.
    await click(byTestId('trinity-onboarding-enter'));
    expect(sceneHarness.openScene).toHaveBeenCalledWith('trinity');
    expect(onClose).toHaveBeenCalled();
  });

  it('awakens even when the LLM step is left empty', async () => {
    await mount();
    await click(byTestId('trinity-onboarding-next'));
    await click(byTestId('trinity-onboarding-next'));
    await click(byTestId('trinity-onboarding-next'));
    await click(byTestId('trinity-onboarding-begin'));

    expect(apiHarness.llmSetConfig).not.toHaveBeenCalled();
    expect(apiHarness.awaken).toHaveBeenCalled();
    expect(byTestId('trinity-onboarding-enter')).toBeTruthy();
  });

  it('surfaces a ceremony failure and stays on the wizard', async () => {
    apiHarness.awaken.mockRejectedValue(new Error('boom'));
    await mount();
    await click(byTestId('trinity-onboarding-next'));
    await click(byTestId('trinity-onboarding-next'));
    await click(byTestId('trinity-onboarding-next'));
    await click(byTestId('trinity-onboarding-begin'));

    expect(byTestId('trinity-onboarding-error').textContent).toContain('boom');
    expect(byTestId('trinity-onboarding-begin')).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('renders nothing while closed', async () => {
    await mount(false);
    expect(container.querySelector('[data-testid="trinity-onboarding"]')).toBeNull();
  });
});
