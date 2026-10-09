import { useMemo } from 'react';
import { useAppSelector } from '@/shared/state/hooks';
import { usePlatform } from '@/shared/tauri';
import { useServices } from './useServices';
import { usePackageInventory } from './usePackageInventory';
import { reconcile } from './reconcile';

/** The Services-tab view model: managed registry + intent × live brew data.
 * On Windows the installed-package inventory is merged in so an id that is
 * installed but owns no SCM service (or whose service name didn't match) still
 * reads as Installed instead of Not installed. */
export function useReconciledServices() {
  const query = useServices();
  const managed = useAppSelector((s) => s.serviceIntent.managed);
  const intent = useAppSelector((s) => s.serviceIntent.intent);
  const activeTab = useAppSelector((s) => s.ui.activeTab);
  const windows = usePlatform() === 'windows';

  // Only the Services tab needs package presence; gate the (heavier) `winget
  // list` probe to it and to machines that actually manage something.
  const inventoryQuery = usePackageInventory(
    windows && activeTab === 'services' && managed.length > 0,
  );
  const inventory = useMemo(() => {
    if (!inventoryQuery.data) return undefined;
    return Object.fromEntries(
      inventoryQuery.data.map((p) => [
        p.name.toLowerCase(),
        { version: p.version, latestVersion: p.latestVersion },
      ]),
    );
  }, [inventoryQuery.data]);

  const services = useMemo(
    () => reconcile(managed, query.data ?? [], intent, inventory),
    [managed, query.data, intent, inventory],
  );

  return { ...query, services };
}
