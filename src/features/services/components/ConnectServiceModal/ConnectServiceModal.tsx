import { useState } from 'react';
import { Plus, Check } from 'lucide-react';
import { useAppDispatch, useAppSelector } from '@/shared/state/hooks';
import { addManaged } from '@/shared/state/serviceIntentSlice';
import { bareFormula, formulaDisplayName } from '@/shared/lib/format';
import { useDebouncedValue } from '@/shared/lib/useDebouncedValue';
import { useSearchFormulae, useServices } from '@/shared/brew';
import { usePlatform } from '@/shared/tauri';
import { Dialog } from '@/shared/ui/Dialog';
import { Spinner } from '@/shared/ui/Spinner';
import { POPULAR_FORMULAE, POPULAR_WINGET_IDS } from './ConnectServiceModal.config';
import type { ConnectServiceModalProps } from './ConnectServiceModal.types';

const SEARCH_DEBOUNCE_MS = 250;

export const ConnectServiceModal = ({ open, onOpenChange }: ConnectServiceModalProps) => {
  const dispatch = useAppDispatch();
  const platform = usePlatform();
  const windows = platform === 'windows';
  const managed = useAppSelector((s) => s.serviceIntent.managed);
  const [query, setQuery] = useState('');
  const debouncedQuery = useDebouncedValue(query, SEARCH_DEBOUNCE_MS);
  const search = useSearchFormulae(debouncedQuery);
  // Everything the package manager already runs as a service on this machine —
  // so a service removed from the list (or installed outside Nexus) can always
  // be added back without knowing its formula/package name. Windows returns
  // every SCM service here, so the list is cut to noteworthy rows.
  const services = useServices();

  // Same bare-name rule the slice dedupes on — otherwise Add looks like a no-op.
  const isAdded = (formula: string) =>
    managed.some((m) => bareFormula(m.formula) === bareFormula(formula));
  const add = (formula: string, displayName?: string) =>
    dispatch(addManaged({ formula, displayName: displayName ?? formulaDisplayName(formula) }));

  const showResults = query.trim().length >= 2;
  const installed = (services.data ?? [])
    .filter((s) => s.noteworthy !== false)
    .map((s) => ({ formula: s.name, label: s.displayName }))
    .filter((s) => !isAdded(s.formula));
  const popular = (windows ? POPULAR_WINGET_IDS : POPULAR_FORMULAE).filter(
    (f) => !installed.some((s) => s.formula === f),
  );

  const chip = (formula: string, label?: string) => (
    <button
      key={formula}
      type="button"
      disabled={isAdded(formula)}
      onClick={() => add(formula, label)}
      className="inline-flex items-center gap-1.5 rounded-md border border-border bg-canvas px-2.5 py-1.5 font-mono text-sm text-fg hover:bg-list disabled:opacity-50"
    >
      {isAdded(formula) ? <Check className="h-3.5 w-3.5" /> : <Plus className="h-3.5 w-3.5" />}
      {label ?? formula}
      {isAdded(formula) && <span className="text-xs text-fg-subtle">· added</span>}
    </button>
  );

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Connect a service"
      description={
        windows
          ? 'Only packages that run as a Windows service can be added.'
          : 'Only formulae Homebrew can run as a service can be added.'
      }
    >
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={
          windows
            ? 'Search WinGet packages (e.g. PostgreSQL)…'
            : 'Search Homebrew formulae (e.g. mysql)…'
        }
        aria-label={windows ? 'Search packages' : 'Search formulae'}
        className="mb-3 h-10 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-fg placeholder:text-fg-subtle"
      />

      <div className="min-h-0 flex-1 overflow-auto">
        {showResults ? (
          search.isPending ? (
            <div className="p-2">
              <Spinner />
            </div>
          ) : search.isError ? (
            <p className="px-1 py-2 text-sm text-danger">
              {search.error instanceof Error ? search.error.message : 'Search failed.'}
            </p>
          ) : (search.data ?? []).length === 0 ? (
            <p className="px-1 py-2 text-sm text-fg-muted">
              {windows ? 'No packages found.' : 'No formulae found.'}
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {(search.data ?? []).slice(0, 40).map((formula) => (
                <li key={formula}>
                  <button
                    type="button"
                    disabled={isAdded(formula)}
                    onClick={() => add(formula)}
                    className="flex w-full items-center justify-between rounded-md px-2.5 py-1.5 text-sm hover:bg-list disabled:opacity-50"
                  >
                    <span className="font-mono text-fg">{formula}</span>
                    {isAdded(formula) ? (
                      <span className="flex items-center gap-1 text-xs text-fg-subtle">
                        <Check className="h-3.5 w-3.5" /> added
                      </span>
                    ) : (
                      <Plus className="h-4 w-4 text-fg-muted" />
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )
        ) : (
          <>
            {installed.length > 0 && (
              <>
                <p className="mb-2 text-xs font-medium text-fg-subtle uppercase">
                  {windows
                    ? 'Installed on this machine · not in your list'
                    : 'Installed on this Mac · not in your list'}
                </p>
                <div className="mb-4 flex flex-wrap gap-2">
                  {installed.map((s) => chip(s.formula, s.label))}
                </div>
              </>
            )}
            <p className="mb-2 text-xs font-medium text-fg-subtle uppercase">Popular services</p>
            <div className="flex flex-wrap gap-2">{popular.map((f) => chip(f))}</div>

            {windows && (
              <div className="mt-4 border-t border-border pt-3">
                <p className="mb-1 text-xs font-medium text-fg-subtle uppercase">
                  Redis on Windows
                </p>
                <p className="mb-2 text-xs leading-relaxed text-fg-muted">
                  There's no official Redis server for Windows. Pick a native alternative that runs
                  as a Windows service:
                </p>
                <div className="flex flex-wrap gap-2">
                  {chip('Redis.Redis', 'Redis.Redis · native port')}
                  {chip('Memurai.MemuraiDeveloper', 'Memurai · Redis-compatible')}
                </div>
                <ul className="mt-2 space-y-1 text-xs text-fg-muted">
                  <li>
                    Docker ·{' '}
                    <code className="font-mono text-fg-subtle">
                      docker run -d -p 6379:6379 redis
                    </code>
                  </li>
                  <li>
                    WSL2 ·{' '}
                    <code className="font-mono text-fg-subtle">sudo apt install redis-server</code>{' '}
                    inside your distro
                  </li>
                </ul>
                <p className="mt-2 text-xs text-fg-muted">
                  RedisInsight is a GUI <em>client</em>, not a server — adding it won't give you
                  Redis.
                </p>
              </div>
            )}
          </>
        )}
      </div>
    </Dialog>
  );
};
