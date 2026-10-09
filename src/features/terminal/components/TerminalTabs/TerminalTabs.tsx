import { Plus, X } from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { useAppDispatch, useAppSelector } from '@/shared/state/hooks';
import {
  openTerminal,
  closeTerminal,
  setActiveTerminal,
  type ShellKind,
} from '@/shared/state/terminalsSlice';
import { usePlatform } from '@/shared/tauri';
import { cn } from '@/shared/lib/cn';

/** The drawer's "+" menu: an interactive shell picker. macOS keeps a single
 * "New shell" entry (login shell); Windows offers PowerShell/CMD explicitly. */
const SHELL_OPTIONS: { shell: Exclude<ShellKind, 'auto'>; title: string }[] = [
  { shell: 'pwsh', title: 'PowerShell' },
  { shell: 'powershell5', title: 'Windows PowerShell 5.1' },
  { shell: 'cmd', title: 'Command Prompt' },
];

export const TerminalTabs = () => {
  const dispatch = useAppDispatch();
  const sessions = useAppSelector((s) => s.terminals.sessions);
  const activeId = useAppSelector((s) => s.terminals.activeId);
  const windows = usePlatform() === 'windows';

  const openShell = (shell: ShellKind, title: string) =>
    dispatch(openTerminal({ formula: 'shell', title, kind: 'cli', shell }));

  const itemCls =
    'flex w-full cursor-default select-none items-center gap-2 rounded-md px-2 py-1.5 text-sm text-fg outline-none data-[highlighted]:bg-list';

  return (
    <div className="flex min-w-0 items-center gap-1 overflow-x-auto">
      <DropdownMenu.Root modal={false}>
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            aria-label="New terminal"
            title="New terminal"
            className="shrink-0 rounded-md p-1 text-fg-muted hover:bg-list hover:text-fg"
          >
            <Plus className="h-4 w-4" />
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            side="bottom"
            align="start"
            sideOffset={4}
            className="nx-pop z-[var(--z-dropdown)] min-w-44 rounded-lg border border-border bg-paper p-1 shadow-md"
          >
            {(windows ? SHELL_OPTIONS : [{ shell: 'auto' as const, title: 'New shell' }]).map(
              ({ shell, title }) => (
                <DropdownMenu.Item
                  key={shell}
                  className={itemCls}
                  onSelect={() => openShell(shell, title)}
                >
                  {title}
                </DropdownMenu.Item>
              ),
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {sessions.map((s) => (
        <div
          key={s.id}
          className={cn(
            'flex shrink-0 items-center gap-1.5 rounded-md py-1 pr-1 pl-2.5 text-xs transition-colors',
            s.id === activeId ? 'bg-list text-fg' : 'text-fg-muted hover:text-fg',
          )}
        >
          <button type="button" onClick={() => dispatch(setActiveTerminal(s.id))}>
            {s.title}
          </button>
          <button
            type="button"
            aria-label={`Close ${s.title}`}
            onClick={() => dispatch(closeTerminal(s.id))}
            className="rounded p-0.5 hover:bg-border"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      ))}
    </div>
  );
};
