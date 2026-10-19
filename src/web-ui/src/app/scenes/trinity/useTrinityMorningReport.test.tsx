// @vitest-environment jsdom

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/infrastructure/i18n/hooks/useI18n', async () => {
  const { createTestI18nT } = await import('@/test/i18nTestUtils');
  return { useI18n: () => ({ t: createTestI18nT('common') }) };
});

import { systemAPI, trinityAPI } from '@/infrastructure/api';
import { useTrinityStore } from './trinityStore';
import { useTrinityMorningReport } from './useTrinityMorningReport';

vi.spyOn(systemAPI, 'sendSystemNotification').mockResolvedValue(undefined);
const morningReportSpy = vi.spyOn(trinityAPI, 'morningReport');

const TODAY = '2026-10-05';
const MORNING = new Date(`${TODAY}T08:00:00`);
const AFTERNOON = new Date(`${TODAY}T14:00:00`);

/** A report the daemon would answer for today. */
const todaysReport = {
  date: TODAY,
  generated_at: MORNING.getTime() / 1000,
  sleep_date: '2026-10-04',
  greeting: '早上好，公子。昨夜整理记忆时想到 2 件事。',
  care_need: 'relatedness',
  care_deficit: 0.4,
  thoughts: [{ id: 's1', content: '想法一', kind: 'consolidation', layer: 'Pattern', strength: 0.55, timestamp: 1 }],
  urges: ['收尾蓝绿切换'],
};

const Probe: React.FC = () => {
  useTrinityMorningReport();
  return null;
};

describe('useTrinityMorningReport', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(MORNING);
    localStorage.clear();
    morningReportSpy.mockReset();
    vi.mocked(systemAPI.sendSystemNotification).mockClear();
    useTrinityStore.setState({
      conn: 'disconnected',
      awakened: false,
      awakenedLocally: false,
      identity: null,
      psi: null,
      cloudStatus: null,
      morningReport: null,
    });
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.useRealTimers();
  });

  const mount = () => act(() => root.render(<Probe />));
  const setAwake = () => useTrinityStore.setState({
    conn: 'connected',
    awakened: true,
    identity: { name: '银月' },
  });

  it('does not pull or notify while dormant', () => {
    useTrinityStore.setState({ conn: 'connected', awakened: false });
    mount();
    expect(morningReportSpy).not.toHaveBeenCalled();
    expect(systemAPI.sendSystemNotification).not.toHaveBeenCalled();
  });

  it('does not pull outside the morning window', () => {
    vi.setSystemTime(AFTERNOON);
    setAwake();
    mount();
    expect(morningReportSpy).not.toHaveBeenCalled();
  });

  it('pulls in the morning window, notifies once, and survives remounts the same day', async () => {
    setAwake();
    morningReportSpy.mockResolvedValue(todaysReport);
    mount();
    await act(async () => {});
    // The pull lands in the store and the toast fires in the same flush.
    expect(morningReportSpy).toHaveBeenCalledTimes(1);
    expect(systemAPI.sendSystemNotification).toHaveBeenCalledTimes(1);
    expect(systemAPI.sendSystemNotification).toHaveBeenCalledWith(
      'Morning report from 银月',
      todaysReport.greeting,
    );

    // Further ticks the same day stay silent.
    act(() => { vi.advanceTimersByTime(60_000); });
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(systemAPI.sendSystemNotification).toHaveBeenCalledTimes(1);

    // Same-day remount (app restart path): the persisted flag suppresses both.
    act(() => root.unmount());
    morningReportSpy.mockClear();
    vi.mocked(systemAPI.sendSystemNotification).mockClear();
    root = createRoot(container);
    mount();
    await act(async () => {});
    expect(morningReportSpy).not.toHaveBeenCalled();
    expect(systemAPI.sendSystemNotification).not.toHaveBeenCalled();
  });

  it('does not notify a report dated other than today', async () => {
    setAwake();
    morningReportSpy.mockResolvedValue({ ...todaysReport, date: '2026-10-04' });
    mount();
    await act(async () => {});
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(systemAPI.sendSystemNotification).not.toHaveBeenCalled();
  });
});
