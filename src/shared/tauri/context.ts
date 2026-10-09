import { useQuery } from '@tanstack/react-query';
import { ipc } from './invoke';
import { CMD } from './commands';
import { appContextSchema } from './schemas';

export const appContextQueryKey = ['app-context'] as const;

/** Machine/brew context. Discovered once by Rust at startup → never stale.
 * brewVersion AND wingetVersion resolve async in Rust (off the startup path) —
 * poll the cheap state-read until either lands, bounded so a permanently-
 * failing probe can't poll forever. */
/** Which OS this app is running on — the switch for every platform-specific
 * string, default and behaviour in the UI. Context loads once at startup and
 * never changes, so this is stable for the component's lifetime; before the
 * first response it reads as macOS (the original platform, so the first paint
 * of a mac install is never wrong and a Windows one corrects immediately). */
export function usePlatform(): 'macos' | 'windows' {
  const { data } = useAppContext();
  return data?.platform === 'windows' ? 'windows' : 'macos';
}

export function useAppContext() {
  return useQuery({
    queryKey: appContextQueryKey,
    queryFn: () => ipc(CMD.GET_APP_CONTEXT, undefined, appContextSchema),
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
    refetchInterval: (query) => {
      if (query.state.dataUpdateCount >= 15) return false;
      const d = query.state.data;
      const brewPending = Boolean(d?.brewBin && !d.brewVersion);
      const wingetPending = Boolean(d?.wingetPath && !d.wingetVersion);
      return brewPending || wingetPending ? 1000 : false;
    },
  });
}
