import { describe, expect, it } from 'vitest';
import {
  addManaged,
  applyPackageIdFixes,
  hydrateServiceIntent,
  renameManaged,
  serviceIntentReducer,
  setIntent,
} from './serviceIntentSlice';

const init = () => serviceIntentReducer(undefined, { type: '@@init' });

describe('serviceIntentSlice', () => {
  it('rewrites a persisted id that no longer exists in winget', () => {
    const state = serviceIntentReducer(
      init(),
      hydrateServiceIntent({
        managed: [
          { formula: 'Redis.Redis-Official', displayName: 'Redis.Redis Official' },
          { formula: 'PostgreSQL.PostgreSQL.17', displayName: 'PostgreSQL 17' },
        ],
        intent: {},
        seeded: true,
      }),
    );
    expect(state.managed.map((m) => m.formula)).toEqual([
      'Redis.Redis',
      'PostgreSQL.PostgreSQL.17',
    ]);
    expect(state.hydrated).toBe(true);
  });

  it('carries link intent across a rewritten id', () => {
    const state = serviceIntentReducer(
      init(),
      hydrateServiceIntent({
        managed: [{ formula: 'Redis.Redis-Official', displayName: 'Redis' }],
        intent: { 'Redis.Redis-Official': 'linked' },
        seeded: true,
      }),
    );
    expect(state.intent).toEqual({ 'Redis.Redis': 'linked' });
  });

  it('leaves verified ids untouched and dedupes on bare name', () => {
    expect(applyPackageIdFixes([{ formula: 'nginxinc.nginx', displayName: 'nginx' }])).toEqual([
      { formula: 'nginxinc.nginx', displayName: 'nginx' },
    ]);
    const once = serviceIntentReducer(
      init(),
      addManaged({ formula: 'Redis.Redis', displayName: 'Redis' }),
    );
    const twice = serviceIntentReducer(
      once,
      addManaged({ formula: 'Redis.Redis', displayName: 'Redis' }),
    );
    expect(twice.managed).toHaveLength(1);
  });

  it('renameManaged moves the card and its intent', () => {
    let state = serviceIntentReducer(init(), addManaged({ formula: 'old', displayName: 'old' }));
    state = serviceIntentReducer(state, setIntent({ formula: 'old', linkState: 'linked' }));
    state = serviceIntentReducer(state, renameManaged({ from: 'old', to: 'new' }));
    expect(state.managed.map((m) => m.formula)).toEqual(['new']);
    expect(state.intent).toEqual({ new: 'linked' });
  });
});
