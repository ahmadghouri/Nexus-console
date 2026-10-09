import { bareFormula } from '@/shared/lib/format';
import type { ManagedService, LinkState } from '@/shared/state/serviceIntentSlice';
import type { ServiceDto } from './schemas';
import type { DisplayStatus, PackageInventory, ReconciledService } from './types';

/** brew status × health → display status. brew is the truth for started/stopped;
 * a started-but-unreachable service is "starting", not green. Windows SCM adds
 * transitional states (start/stop pending) that map onto the same chips. */
export function mapStatus(dto: ServiceDto | undefined): DisplayStatus {
  if (!dto) return 'notInstalled';
  switch (dto.status) {
    case 'started':
      return dto.healthy === false ? 'starting' : 'running';
    case 'start_pending':
      return 'starting'; // SCM START_PENDING / CONTINUE_PENDING
    case 'stop_pending':
      return 'stopping'; // SCM STOP_PENDING / PAUSE_PENDING
    case 'stopping':
      return 'stopping'; // synthetic: only ever set by the stop optimistic update
    case 'error':
      return 'error';
    default:
      return 'stopped'; // stopped | none | scheduled | paused
  }
}

/** A winget id (`PostgreSQL.PostgreSQL.17`) manages the SCM service it
 * installed (`postgresql-x64-17`) — the same ≥4-char segment rule Rust's
 * service_hint uses in package_info, so both ends agree on what counts as a
 * match. Short segments are skipped so `Git.Git` can never land on a service. */
function wingetIdMatchesService(id: string, dto: ServiceDto): boolean {
  const name = dto.name.toLowerCase();
  const display = (dto.displayName ?? '').toLowerCase();
  return id
    .split('.')
    .some(
      (seg) =>
        seg.length >= 4 &&
        (name.includes(seg.toLowerCase()) || display.includes(seg.toLowerCase())),
    );
}

/** Merge the managed registry (+ user intent) with live brew data and, on
 * Windows, the installed-package inventory. `status` stays derived from the
 * service record alone (so tray/session/badge consumers are unaffected); the
 * inventory only enriches an entry with `installed`/version/upgrade info. */
export function reconcile(
  managed: ManagedService[],
  brewList: ServiceDto[],
  intent: Record<string, LinkState>,
  inventory?: PackageInventory,
): ReconciledService[] {
  // Keyed on the bare name: a service added from search is tap-qualified
  // (`user/repo/name@1`) while brew reports it bare (`name@1`) — matching on the
  // raw string showed an installed service as "Not installed".
  // ponytail: two taps shipping the same formula name would collide; key on the
  // full name if that ever happens for real.
  const byName = new Map(brewList.map((d) => [bareFormula(d.name), d]));
  return managed.map((m) => {
    // Exact name first (brew + SCM names), then the winget-id segment rule —
    // Windows manages SCM services under the winget id they were installed with.
    const dto =
      byName.get(bareFormula(m.formula)) ??
      (m.formula.includes('.')
        ? brewList.find((d) => wingetIdMatchesService(m.formula, d))
        : undefined);
    const serviceInstalled = dto !== undefined;
    const inv = inventory?.[m.formula.toLowerCase()];
    const installed = serviceInstalled || inv !== undefined;
    const version = inv?.version ?? null;
    const latestVersion = inv?.latestVersion ?? null;
    const updateAvailable = installed && latestVersion !== null && latestVersion !== version;
    return {
      formula: m.formula,
      displayName: m.displayName,
      status: mapStatus(dto),
      linkState: intent[m.formula] ?? 'unlinked',
      pid: dto?.pid ?? null,
      cpu: dto?.cpu ?? null,
      memoryBytes: dto?.memoryBytes ?? null,
      uptimeSecs: dto?.uptimeSecs ?? null,
      port: dto?.port ?? null,
      healthy: dto?.healthy ?? null,
      plist: dto?.plist ?? null,
      installed,
      serviceInstalled,
      version,
      latestVersion,
      updateAvailable,
    };
  });
}
