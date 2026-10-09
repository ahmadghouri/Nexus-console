import type { ServiceStatus } from '@/shared/ui/StatusChip';
import type { LinkState } from '@/shared/state/serviceIntentSlice';

/** Display status derives from brew status × health (started+unreachable → starting). */
export type DisplayStatus = ServiceStatus;

/** A managed service enriched with live brew data — what the Services tab renders. */
export interface ReconciledService {
  formula: string;
  displayName: string;
  status: DisplayStatus;
  linkState: LinkState;
  pid: number | null;
  cpu: number | null;
  memoryBytes: number | null;
  uptimeSecs: number | null;
  port: number | null;
  healthy: boolean | null;
  plist: string | null;
  /** The package is present in the package manager (brew/SCM/WinGet inventory).
   * On macOS this mirrors `serviceInstalled`; on Windows a winget id can be
   * installed (Git.Git) without owning an SCM service, so the two differ. */
  installed: boolean;
  /** A live service record exists for this entry (an SCM row is matched, or a
   * brew service). Gates the metrics line, Link chip and power actions. */
  serviceInstalled: boolean;
  /** Installed version from the package inventory (Windows; null on macOS). */
  version: string | null;
  /** Newest available version, only when an upgrade exists (Windows). */
  latestVersion: string | null;
  /** An upgrade is available for this installed package. */
  updateAvailable: boolean;
}

/** One package's presence/version, keyed by id in `PackageInventory`. */
export interface PackageInventoryEntry {
  version: string;
  latestVersion: string | null;
}

/** Installed-package inventory keyed by lower-cased package id. */
export type PackageInventory = Record<string, PackageInventoryEntry>;
