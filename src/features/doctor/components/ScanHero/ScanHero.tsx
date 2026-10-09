import { Code, Cog, Globe, HardDrive, Power, ShieldCheck, Stethoscope } from 'lucide-react';
import { usePlatform } from '@/shared/tauri';
import { cn } from '@/shared/lib/cn';
import { Button } from '@/shared/ui/Button';

interface Area {
  id: string;
  icon: typeof Cog;
  label: string;
  desc: string;
  badge: string;
}

/** What one scan covers — shown instead of blank tabs on first run. The lists
 * are platform-true: Windows has no Browser probes (mac-only `open`/`du`
 * work), Disk has no dev-cache sweep there, and Startup means registry/run-key
 * startup apps, not launch agents. */
const AREAS_MAC: Area[] = [
  {
    id: 'system',
    icon: Cog,
    label: 'System',
    desc: 'Low Power Mode, memory pressure, Time Machine',
    badge: 'bg-accent-soft text-accent',
  },
  {
    id: 'vscode',
    icon: Code,
    label: 'VSCode',
    desc: 'Extensions, orphan caches, database bloat',
    badge: 'bg-running-soft text-running',
  },
  {
    id: 'browser',
    icon: Globe,
    label: 'Browser',
    desc: 'Memory, extension cost, caches',
    badge: 'bg-accent-soft text-accent',
  },
  {
    id: 'disk',
    icon: HardDrive,
    label: 'Disk',
    desc: 'Free space, dev caches, ollama models',
    badge: 'bg-warn-soft text-warn',
  },
  {
    id: 'startup',
    icon: Power,
    label: 'Startup',
    desc: 'Third-party launch agents',
    badge: 'bg-running-soft text-running',
  },
];

const AREAS_WIN: Area[] = [
  {
    id: 'system',
    icon: Cog,
    label: 'System',
    desc: 'Battery saver, memory pressure, pending reboot',
    badge: 'bg-accent-soft text-accent',
  },
  {
    id: 'vscode',
    icon: Code,
    label: 'VSCode',
    desc: 'Extensions, orphan caches, database bloat',
    badge: 'bg-running-soft text-running',
  },
  {
    id: 'disk',
    icon: HardDrive,
    label: 'Disk',
    desc: 'Free space, ollama models',
    badge: 'bg-warn-soft text-warn',
  },
  {
    id: 'startup',
    icon: Power,
    label: 'Startup',
    desc: 'Startup apps',
    badge: 'bg-running-soft text-running',
  },
];

/** First-run hero: teaches what the Doctor checks and offers the scans.
 * Deep scan is macOS-only (the sweep it names is `find`/`du` work), so the
 * second button never renders on Windows — the quick scan there already runs
 * every probe available on the machine. */
export const ScanHero = ({
  onScan,
  onDeepScan,
}: {
  onScan: () => void;
  onDeepScan: () => void;
}) => {
  const windows = usePlatform() === 'windows';
  const areas = windows ? AREAS_WIN : AREAS_MAC;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex min-h-full max-w-4xl flex-col items-center justify-center gap-6 p-8">
        <div className="flex flex-col items-center gap-3 text-center">
          <div className="flex h-16 w-16 items-center justify-center rounded-full bg-accent-soft">
            <Stethoscope className="h-7 w-7 text-accent" />
          </div>
          <h2 className="text-xl font-semibold text-fg">
            {windows ? "Check your machine's health" : "Check your Mac's health"}
          </h2>
          <p className="max-w-md text-sm leading-relaxed text-fg-muted">
            One read-only scan across key areas. Nothing changes without your confirmation.
          </p>
        </div>

        <div className="flex w-full items-center gap-3" aria-hidden>
          <span className="h-px flex-1 bg-border" />
          <span className="text-xs font-medium text-fg-subtle">Scan areas</span>
          <span className="h-px flex-1 bg-border" />
        </div>

        <div className="grid w-full gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {areas.map(({ icon: Icon, label, desc, badge }) => (
            <div
              key={label}
              className="flex h-20 items-center gap-3 rounded-xl border border-border bg-list px-4"
            >
              <span
                className={cn(
                  'flex h-9 w-9 shrink-0 items-center justify-center rounded-full',
                  badge,
                )}
              >
                <Icon className="h-4 w-4" />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-semibold text-fg">{label}</span>
                <span className="line-clamp-2 block text-xs leading-relaxed text-fg-muted">
                  {desc}
                </span>
              </span>
            </div>
          ))}
        </div>

        <div className="flex flex-col items-center gap-3">
          <div className="flex items-center gap-2">
            <Button onClick={onScan}>
              <Stethoscope className="h-4 w-4" />
              Scan
            </Button>
            {!windows && (
              <Button variant="secondary" onClick={onDeepScan}>
                <HardDrive className="h-4 w-4" />
                Deep scan
              </Button>
            )}
          </div>
          {!windows && (
            <p className="text-xs text-fg-subtle">
              Deep scan also sweeps node_modules and dev caches — slower, but finds the most disk.
            </p>
          )}
        </div>

        <div className="flex w-full items-center gap-3 rounded-xl border border-border bg-list px-4 py-3">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-accent-soft text-accent">
            <ShieldCheck className="h-4 w-4" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-sm font-semibold text-fg">Safe &amp; Read-only</span>
            <span className="block text-xs leading-relaxed text-fg-muted">
              This tool only reads system information. No changes are made without your
              confirmation.
            </span>
          </span>
        </div>
      </div>
    </div>
  );
};
