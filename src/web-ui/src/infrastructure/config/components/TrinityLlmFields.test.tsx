// @vitest-environment jsdom

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const apiHarness = vi.hoisted(() => ({
  llmTestConnection: vi.fn(),
}));

vi.mock('@/infrastructure/api', () => ({
  trinityAPI: apiHarness,
}));

import TrinityLlmFields, { type TrinityLlmFieldsValue } from './TrinityLlmFields';

/** Controlled host so the component's onChange patches are observable. */
const Host: React.FC<{ onValue?: (value: TrinityLlmFieldsValue) => void }> = ({ onValue }) => {
  const [value, setValue] = React.useState<TrinityLlmFieldsValue>({ apiUrl: '', model: '', apiKey: '' });
  return (
    <TrinityLlmFields
      value={value}
      onChange={patch => {
        setValue(previous => {
          const next = { ...previous, ...patch };
          onValue?.(next);
          return next;
        });
      }}
    />
  );
};

describe('TrinityLlmFields', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    apiHarness.llmTestConnection.mockReset();
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  const mount = (onValue?: (value: TrinityLlmFieldsValue) => void) => act(() => {
    root.render(<Host onValue={onValue} />);
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

  it('fills url and model from a provider preset', async () => {
    const seen: TrinityLlmFieldsValue[] = [];
    await mount(value => seen.push(value));
    await click(byTestId('trinity-llm-preset-deepseek'));

    expect((byTestId('trinity-llm-api-url') as HTMLInputElement).value).toBe('https://api.deepseek.com/v1');
    expect((byTestId('trinity-llm-model') as HTMLInputElement).value).toBe('deepseek-chat');
    expect(seen.at(-1)).toMatchObject({ apiUrl: 'https://api.deepseek.com/v1', model: 'deepseek-chat' });
  });

  it('tests the connection and reports the outcome inline', async () => {
    apiHarness.llmTestConnection.mockResolvedValue({ ok: true, latency_ms: 42 });
    const seen: TrinityLlmFieldsValue[] = [];
    await mount(value => seen.push(value));

    typeInto(byTestId('trinity-llm-api-url'), 'https://api.deepseek.com/v1');
    typeInto(byTestId('trinity-llm-model'), 'deepseek-chat');
    typeInto(byTestId('trinity-llm-api-key'), 'sk-test');
    await click(byTestId('trinity-llm-test'));

    expect(apiHarness.llmTestConnection).toHaveBeenCalledWith({
      api_url: 'https://api.deepseek.com/v1', model: 'deepseek-chat', api_key: 'sk-test',
    });
    expect(byTestId('trinity-llm-test-result').textContent).toBe('trinity.engine.connectionOk');
  });

  it('reports a failed connection without throwing', async () => {
    apiHarness.llmTestConnection.mockResolvedValue({ ok: false, error: '401 unauthorized' });
    await mount();
    typeInto(byTestId('trinity-llm-api-url'), 'https://api.deepseek.com/v1');
    typeInto(byTestId('trinity-llm-model'), 'deepseek-chat');
    typeInto(byTestId('trinity-llm-api-key'), 'sk-bad');
    await click(byTestId('trinity-llm-test'));

    expect(byTestId('trinity-llm-test-result').textContent).toBe('401 unauthorized');
  });
});
