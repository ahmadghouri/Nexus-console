import { useEffect } from 'react';
import { useAppDispatch, useAppSelector } from '@/shared/state/hooks';
import { seedManaged } from '@/shared/state/serviceIntentSlice';
import { formulaDisplayName } from '@/shared/lib/format';
import { useServices } from './useServices';

/** First run (per machine): seed the managed list from the services that
 * actually exist here — no hardcoded defaults. Runs once, after intent hydrates.
 * Windows lists every SCM service (hundreds, each tagged `noteworthy`), so only
 * noteworthy rows seed the list; displayName comes from the SCM metadata when
 * present — formulaDisplayName is a brew-name fallback, not a winget-id parser. */
export function useSeedManagedServices(): void {
  const dispatch = useAppDispatch();
  const hydrated = useAppSelector((s) => s.serviceIntent.hydrated);
  const seeded = useAppSelector((s) => s.serviceIntent.seeded);
  const { data } = useServices();

  useEffect(() => {
    if (hydrated && !seeded && data && data.length > 0) {
      dispatch(
        seedManaged(
          data
            .filter((d) => d.noteworthy !== false)
            .map((d) => ({
              formula: d.name,
              displayName: d.displayName ?? formulaDisplayName(d.name),
            })),
        ),
      );
    }
  }, [hydrated, seeded, data, dispatch]);
}
