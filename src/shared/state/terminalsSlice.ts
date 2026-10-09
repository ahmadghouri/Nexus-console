import { createSlice, nanoid, type PayloadAction } from '@reduxjs/toolkit';
import { env } from '@/config/env';

export type TerminalKind = 'cli' | 'logs';

/** Mirrors pty.ts ShellId — the "+ " menu's shell picker (Windows only). */
export type ShellKind = 'auto' | 'pwsh' | 'powershell5' | 'cmd';

export interface TerminalSession {
  id: string;
  formula: string;
  title: string;
  kind: TerminalKind;
  shell: ShellKind;
}

interface TerminalsState {
  sessions: TerminalSession[];
  activeId: string | null;
  drawerOpen: boolean;
}

const initialState: TerminalsState = { sessions: [], activeId: null, drawerOpen: false };

const terminalsSlice = createSlice({
  name: 'terminals',
  initialState,
  reducers: {
    // The ONE cross-feature seam: Services dispatches this; the terminal feature reacts.
    openTerminal: {
      reducer(state, action: PayloadAction<TerminalSession>) {
        const existing = state.sessions.find(
          (s) =>
            s.formula === action.payload.formula &&
            s.kind === action.payload.kind &&
            s.shell === action.payload.shell,
        );
        if (existing) {
          state.activeId = existing.id;
        } else if (state.sessions.length < env.VITE_PTY_CAP) {
          state.sessions.push(action.payload);
          state.activeId = action.payload.id;
        }
        state.drawerOpen = true;
      },
      prepare(input: { formula: string; title: string; kind: TerminalKind; shell?: ShellKind }) {
        return { payload: { ...input, id: nanoid(), shell: input.shell ?? 'auto' } };
      },
    },
    closeTerminal(state, action: PayloadAction<string>) {
      const idx = state.sessions.findIndex((s) => s.id === action.payload);
      if (idx >= 0) state.sessions.splice(idx, 1);
      if (state.activeId === action.payload) {
        state.activeId = state.sessions.at(-1)?.id ?? null;
      }
      if (state.sessions.length === 0) state.drawerOpen = false;
    },
    setActiveTerminal(state, action: PayloadAction<string>) {
      state.activeId = action.payload;
      state.drawerOpen = true;
    },
    setDrawerOpen(state, action: PayloadAction<boolean>) {
      state.drawerOpen = action.payload;
    },
  },
});

export const { openTerminal, closeTerminal, setActiveTerminal, setDrawerOpen } =
  terminalsSlice.actions;
export const terminalsReducer = terminalsSlice.reducer;
