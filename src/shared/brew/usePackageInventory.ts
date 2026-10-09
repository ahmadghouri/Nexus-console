import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { ipc, CMD } from '@/shared/tauri';
import type { PackageInventory } from './types';

/** The slice of `list_packages` the Services view needs — `outdated`/vendor
 * columns are irrelevant here, so a local subset schema keeps shared/brew from
 * importing upward into features/packages. A DISTINCT query key (not
 * `['packages']`) avoids clobbering the Packages tab's fuller DTO in the shared
 * cache; invalidating `['packages']` still refreshes both (prefix match). */
const inventorySchema = z.array(
  z.object({
    name: z.string(),
    version: z.string(),
    latestVersion: z.string().nullable(),
  }),
);
type InventoryRow = z.infer<typeof inventorySchema>[number];

/** Installed-package inventory for the Services tab (Windows). Off on macOS —
 * there the service record already tells the whole installed story. */
export function usePackageInventory(enabled: boolean) {
  const query = useQuery({
    queryKey: ['packages', 'inventory'],
    queryFn: () => ipc(CMD.LIST_PACKAGES, undefined, inventorySchema),
    enabled,
    staleTime: 60_000,
  });

  const inventory = useMemo<PackageInventory | undefined>(() => {
    if (!query.data) return undefined;
    const map: PackageInventory = {};
    for (const p of query.data as InventoryRow[]) {
      map[p.name.toLowerCase()] = { version: p.version, latestVersion: p.latestVersion };
    }
    return map;
  }, [query.data]);

  return { ...query, inventory };
}
