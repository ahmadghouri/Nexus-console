import { useEffect, useRef } from 'react';
import { useVisibleTabs } from '@/shared/modules';
import { useAppDispatch } from '@/shared/state/hooks';
import { setActiveTab } from '@/shared/state/uiSlice';

/** Global shortcuts: Cmd/Ctrl+1..N switch tabs in sidebar order, Cmd/Ctrl+,
 * opens Settings (the standard "Preferences" shortcut), Cmd/Ctrl+F focuses the
 * active tab's search/filter input, if it has one (marked `data-nx-page-search`;
 * Sessions/Settings have none, so the shortcut is a no-op there). Cmd on macOS,
 * Ctrl on Windows — either modifier is accepted so muscle memory carries over.
 *
 * The numbers are derived from the VISIBLE tabs, so a disabled module never
 * leaves a dead key and the run is always contiguous. */
export function useKeyboardShortcuts(): void {
  const dispatch = useAppDispatch();
  const visibleTabs = useVisibleTabs();
  // Read through a ref so the listener is registered once, not re-subscribed on
  // every render (the hook returns a fresh array each time).
  const tabsRef = useRef(visibleTabs);
  tabsRef.current = visibleTabs;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((!e.metaKey && !e.ctrlKey) || e.altKey) return;

      const tabs = tabsRef.current;
      const nth = Number(e.key);
      if (Number.isInteger(nth) && nth >= 1 && nth <= tabs.length) {
        e.preventDefault();
        dispatch(setActiveTab(tabs[nth - 1]));
        return;
      }
      if (e.key === ',') {
        e.preventDefault();
        dispatch(setActiveTab('settings'));
        return;
      }
      if (e.key.toLowerCase() === 'f') {
        const input = document.querySelector<HTMLInputElement>('[data-nx-page-search]');
        if (!input) return;
        e.preventDefault();
        input.focus();
        input.select();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [dispatch]);
}
