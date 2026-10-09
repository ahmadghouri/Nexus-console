import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { MoreVertical, Info, PlusCircle, Trash2 } from 'lucide-react';
import { useAppDispatch } from '@/shared/state/hooks';
import { addManaged } from '@/shared/state/serviceIntentSlice';
import { formulaDisplayName } from '@/shared/lib/format';
import { usePlatform } from '@/shared/tauri';
import type { PackageDto } from '@/features/packages/api/packages.schema';

const itemCls =
  'flex w-full cursor-default items-center gap-2 rounded-md px-2.5 py-1.5 text-sm outline-none data-[highlighted]:bg-list';

interface Props {
  pkg: PackageDto;
  isServiceCapable: boolean;
  onInfo: () => void;
  onUninstall: () => void;
}

export const PackageActions = ({ pkg, isServiceCapable, onInfo, onUninstall }: Props) => {
  const dispatch = useAppDispatch();
  // winget's `cask` rows are ARP/MSIX inventory — display-only (ids with a
  // backslash fail validate_package_id, source-less ids aren't in winget's
  // sources), so Uninstall is never offered for them on Windows. macOS casks
  // are real `brew uninstall --cask` targets and keep the item.
  const canUninstall = !(usePlatform() === 'windows' && pkg.kind === 'cask');

  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          aria-label="Package actions"
          className="shrink-0 rounded-md p-1.5 text-fg-muted transition-colors hover:bg-list hover:text-fg"
        >
          <MoreVertical className="h-4 w-4" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={4}
          className="nx-pop z-[var(--z-dropdown)] min-w-48 rounded-lg border border-border bg-paper p-1 shadow-md"
        >
          <DropdownMenu.Item className={`${itemCls} text-fg`} onSelect={onInfo}>
            <Info className="h-4 w-4" />
            View info
          </DropdownMenu.Item>
          {isServiceCapable && (
            <DropdownMenu.Item
              className={`${itemCls} text-fg`}
              onSelect={() =>
                dispatch(
                  addManaged({
                    formula: pkg.name,
                    displayName: pkg.displayName ?? formulaDisplayName(pkg.name),
                  }),
                )
              }
            >
              <PlusCircle className="h-4 w-4" />
              Add to Services
            </DropdownMenu.Item>
          )}
          {canUninstall && (
            <DropdownMenu.Item className={`${itemCls} text-danger`} onSelect={onUninstall}>
              <Trash2 className="h-4 w-4" />
              Uninstall
            </DropdownMenu.Item>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
};
