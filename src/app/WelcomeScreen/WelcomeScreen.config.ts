import type { ModuleId } from '@/shared/modules';

/** One-click starting points. Unavailable ones are filtered out at render. */
export const PRESETS: { label: string; modules: ModuleId[] }[] = [
  { label: 'Everything', modules: ['homebrew', 'ports', 'doctor', 'claude'] },
  { label: 'Homebrew only', modules: ['homebrew'] },
  { label: 'Claude Chats only', modules: ['claude'] },
];

/** Preset labels are authored for macOS; on Windows the Homebrew preset names
 * the package manager that actually runs there. */
export const presetLabel = (label: string, platform: string): string =>
  platform === 'windows' && label === 'Homebrew only' ? 'WinGet only' : label;
