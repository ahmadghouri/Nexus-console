import {
  Play,
  Square,
  Link2,
  Unlink,
  Download,
  TerminalSquare,
  ScrollText,
  ArrowUpCircle,
  Check,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useAppDispatch } from '@/shared/state/hooks';
import { setIntent } from '@/shared/state/serviceIntentSlice';
import { openTerminal } from '@/shared/state/terminalsSlice';
import { markUserStop } from '@/shared/lib/userStops';
import { Button } from '@/shared/ui/Button';
import type { ButtonProps } from '@/shared/ui/Button';
import { Spinner } from '@/shared/ui/Spinner/Spinner';
import { useSetLinkIntent, useInstallFormula } from '@/shared/brew';
// Deep import — the packages barrel re-exports PackagesPage and would chain the
// whole packages chunk into the default services tab.
import { useUpgradePackage } from '@/features/packages/api/packageMutations';
import { useServiceControls } from '@/features/services/useServiceControls';
import { powerState } from './powerState';
import type { ServiceActionsProps } from './ServiceActions.types';

interface ActionItem {
  key: string;
  label: string;
  Icon: LucideIcon;
  onClick: () => void;
  variant: ButtonProps['variant'];
  disabled: boolean;
  loading: boolean;
}

export const ServiceActions = ({ service, compact = false }: ServiceActionsProps) => {
  const dispatch = useAppDispatch();
  const { startMut, stopMut, start, stop } = useServiceControls();
  const relink = useSetLinkIntent();
  const install = useInstallFormula();
  const upgrade = useUpgradePackage();

  const linked = service.linkState === 'linked';
  const isRunning = service.status === 'running' || service.status === 'starting';
  // Button state derives from the REAL service status (+ the in-flight mutation),
  // so the loader tracks the actual start/stop transition, not a guess. See
  // powerState.ts.
  const power = powerState(service.status, startMut.isPending, stopMut.isPending);
  // Other actions lock while any brew op runs or the service is mid-transition.
  const busy = power.loading || relink.isPending || install.isPending;

  const renderActions = (actions: ActionItem[]) => {
    if (compact) {
      return (
        <div className="flex flex-nowrap items-center gap-1">
          {actions.map(({ key, label, Icon, onClick, variant, disabled, loading }) => (
            <Button
              key={key}
              variant={variant}
              size="icon"
              disabled={disabled}
              onClick={onClick}
              aria-label={label}
              title={label}
            >
              {loading ? (
                <Spinner className="h-4 w-4 text-current" />
              ) : (
                <Icon className="h-4 w-4" />
              )}
            </Button>
          ))}
        </div>
      );
    }
    return (
      // Equal-width buttons fill the footer in one clean row. No `min-w-0`/`truncate`,
      // so flex-1 never shrinks a button below its label — no more "St…"/"Lo…".
      // The grid's min card width (ServiceGrid) guarantees all four fit.
      <div className="flex items-center gap-1.5">
        {actions.map(({ key, label, Icon, onClick, variant, disabled, loading }) => (
          <Button
            key={key}
            variant={variant}
            size="sm"
            disabled={disabled}
            onClick={onClick}
            className="flex-1 gap-1.5 px-2"
          >
            {loading ? (
              <Spinner className="h-3.5 w-3.5 shrink-0 text-current" />
            ) : (
              <Icon className="h-3.5 w-3.5 shrink-0" />
            )}
            {label}
          </Button>
        ))}
      </div>
    );
  };

  if (!service.installed) {
    return (
      <Button size="sm" disabled={busy} onClick={() => install.mutate({ name: service.formula })}>
        <Download className="h-4 w-4" />
        Install
      </Button>
    );
  }

  // Installed package that owns no service (Git.Git, VS Code, …): nothing to
  // start/stop — offer an upgrade when one exists, otherwise just confirm the
  // install instead of the old misleading "Not installed" + failing Install.
  if (!service.serviceInstalled) {
    return renderActions([
      ...(service.updateAvailable
        ? ([
            {
              key: 'upgrade',
              label: 'Upgrade',
              Icon: ArrowUpCircle,
              onClick: () => upgrade.mutate({ name: service.formula }),
              variant: 'primaryOutline',
              disabled: upgrade.isPending,
              loading: upgrade.isPending,
            },
          ] satisfies ActionItem[])
        : []),
      {
        key: 'installed',
        label: 'Installed',
        Icon: Check,
        onClick: () => {},
        variant: 'secondary',
        disabled: true,
        loading: false,
      },
    ]);
  }

  const toggleLink = () => {
    const next = linked ? 'unlinked' : 'linked';
    dispatch(setIntent({ formula: service.formula, linkState: next }));
    if (isRunning) {
      markUserStop(service.formula); // relink stops+starts — not a crash
      relink.mutate({ name: service.formula, linked: !linked });
    }
  };

  // Intent + user-stop bookkeeping live in useServiceControls (shared with bulk).
  // Errors surface via the global toast; catch just avoids an unhandled rejection.
  const handleStart = () => void start(service).catch(() => {});
  const handleStop = () => void stop(service).catch(() => {});

  const openCli = () =>
    dispatch(openTerminal({ formula: service.formula, title: service.displayName, kind: 'cli' }));
  const openLogs = () =>
    dispatch(
      openTerminal({
        formula: service.formula,
        title: `${service.displayName} · logs`,
        kind: 'logs',
      }),
    );

  const powerAction: ActionItem =
    power.face === 'stop'
      ? {
          key: 'stop',
          label: 'Stop',
          Icon: Square,
          onClick: handleStop,
          variant: 'dangerOutline',
          disabled: power.disabled || relink.isPending || install.isPending,
          loading: power.loading,
        }
      : {
          key: 'start',
          label: 'Start',
          Icon: Play,
          onClick: handleStart,
          variant: 'primaryOutline',
          disabled: power.disabled || relink.isPending || install.isPending,
          loading: power.loading,
        };

  const actions: ActionItem[] = [
    powerAction,
    {
      key: 'link',
      label: linked ? 'Unlink' : 'Link',
      Icon: linked ? Unlink : Link2,
      onClick: toggleLink,
      variant: 'secondary',
      disabled: busy,
      loading: false,
    },
    {
      key: 'cli',
      label: 'CLI',
      Icon: TerminalSquare,
      onClick: openCli,
      variant: 'secondary',
      disabled: false,
      loading: false,
    },
    {
      key: 'logs',
      label: 'Logs',
      Icon: ScrollText,
      onClick: openLogs,
      variant: 'ghost',
      disabled: false,
      loading: false,
    },
  ];

  return renderActions(actions);
};
