/**
 * Morning-report notification scheduler (T8).
 *
 * The daemon never pushes: "scheduled delivery" is the UI pulling
 * `morning.report` at morning time (the first pull of the day lazily
 * generates it daemon-side). This hook adds no daemon polling of its own —
 * the sidebar being entry already refreshes the shared store every 30s;
 * here we only tick a local clock and act when all gates pass:
 *
 * - phase === 'awake'  — dormant/offline means the being does not exist yet,
 *    so there is no morning report and no notification (three-phase machine);
 * - local time inside the morning window — a "good morning" toast outside
 *    the morning is noise;
 * - not yet notified today — once per calendar day, persisted so an app
 *    restart does not re-notify.
 *
 * ponytail: the window is hardcoded and the once-per-day dedup ignores
 * whether the report was already read in the scene (opening the Trinity
 * scene inside the morning window may coincide with the toast); upgrade
 * path = user-configurable notify time + a read-signal from the scene card.
 */

import { useEffect, useState } from 'react';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import { systemAPI } from '@/infrastructure/api';
import { useTrinityStore, useTrinityPhase } from './trinityStore';

/** Local clock tick; the store data arrives via the sidebar's own polling. */
const CLOCK_TICK_MS = 60_000;
/** Morning window in local hours: sleep consolidation runs at 03:30 by default. */
const MORNING_START_HOUR = 5;
const MORNING_END_HOUR = 12;
/** localStorage key holding the last notified date (YYYY-MM-DD). */
const NOTIFIED_KEY = 'trinity.morning.notifiedDate';

function localDateString(now: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

export function useTrinityMorningReport(): void {
  const { t } = useI18n('common');
  const phase = useTrinityPhase();
  const morningReport = useTrinityStore(s => s.morningReport);
  const identityName = useTrinityStore(s => s.identity?.name);
  const loadMorning = useTrinityStore(s => s.loadMorning);

  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, []);

  const today = localDateString(now);
  const hour = now.getHours();
  const inMorningWindow = hour >= MORNING_START_HOUR && hour < MORNING_END_HOUR;

  useEffect(() => {
    if (phase !== 'awake' || !inMorningWindow) return;
    if (localStorage.getItem(NOTIFIED_KEY) === today) return;

    if (morningReport?.date === today) {
      // Report already in the store (we pulled it, or the scene card did).
      localStorage.setItem(NOTIFIED_KEY, today);
      const name = identityName || t('trinity.being.title');
      void systemAPI.sendSystemNotification(
        t('trinity.morning.notifyTitle', { name }),
        morningReport.greeting || t('trinity.morning.notifyBody'),
      );
      return;
    }
    // Not fetched yet this session: pull it (lazily generates on the daemon).
    // A failed pull resolves null and the next tick retries.
    void loadMorning();
  }, [phase, inMorningWindow, today, morningReport, identityName, loadMorning, t]);
}
