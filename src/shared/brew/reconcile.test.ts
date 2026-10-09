import { test, expect } from 'vitest';
import { mapStatus, reconcile } from './reconcile';
import type { ServiceDto } from './schemas';

const dto = (over: Partial<ServiceDto>): ServiceDto => ({
  name: 'redis',
  status: 'started',
  pid: 1,
  cpu: 0,
  memoryBytes: 0,
  uptimeSecs: 0,
  port: 6379,
  healthy: true,
  plist: null,
  ...over,
});

test('mapStatus: started + healthy → running', () => {
  expect(mapStatus(dto({ status: 'started', healthy: true }))).toBe('running');
});

test('mapStatus: started + unreachable → starting (not green)', () => {
  expect(mapStatus(dto({ status: 'started', healthy: false }))).toBe('starting');
});

test('mapStatus: none/stopped → stopped', () => {
  expect(mapStatus(dto({ status: 'none' }))).toBe('stopped');
  expect(mapStatus(dto({ status: 'stopped' }))).toBe('stopped');
});

test('mapStatus: Windows SCM pending states → starting/stopping', () => {
  expect(mapStatus(dto({ status: 'start_pending' }))).toBe('starting');
  expect(mapStatus(dto({ status: 'stop_pending' }))).toBe('stopping');
  expect(mapStatus(dto({ status: 'error' }))).toBe('error');
});

test('mapStatus: missing dto → notInstalled', () => {
  expect(mapStatus(undefined)).toBe('notInstalled');
});

test('reconcile: managed formula absent from brew → notInstalled + unlinked', () => {
  const [row] = reconcile([{ formula: 'nginx', displayName: 'Nginx' }], [], {});
  expect(row.status).toBe('notInstalled');
  expect(row.linkState).toBe('unlinked');
  expect(row.installed).toBe(false);
  expect(row.serviceInstalled).toBe(false);
});

test('reconcile: without inventory, installed mirrors service presence (macOS)', () => {
  const [row] = reconcile([{ formula: 'redis', displayName: 'Redis' }], [dto({})], {});
  expect(row.installed).toBe(true);
  expect(row.serviceInstalled).toBe(true);
  expect(row.version).toBeNull();
  expect(row.updateAvailable).toBe(false);
});

test('reconcile: inventory marks a serviceless package installed', () => {
  const [row] = reconcile(
    [{ formula: 'Git.Git', displayName: 'Git' }],
    [],
    {},
    {
      'git.git': { version: '2.51.2', latestVersion: null },
    },
  );
  // status still derives from the service record — but the card shows Installed.
  expect(row.status).toBe('notInstalled');
  expect(row.installed).toBe(true);
  expect(row.serviceInstalled).toBe(false);
  expect(row.version).toBe('2.51.2');
  expect(row.updateAvailable).toBe(false);
});

test('reconcile: inventory newer version → updateAvailable', () => {
  const [row] = reconcile(
    [{ formula: 'Redis.Redis', displayName: 'Redis' }],
    [],
    {},
    { 'redis.redis': { version: '3.0.504', latestVersion: '3.2.0' } },
  );
  expect(row.installed).toBe(true);
  expect(row.version).toBe('3.0.504');
  expect(row.latestVersion).toBe('3.2.0');
  expect(row.updateAvailable).toBe(true);
});

test('reconcile: inventory latest equal to installed → no update offered', () => {
  const [row] = reconcile(
    [{ formula: 'Git.Git', displayName: 'Git' }],
    [],
    {},
    { 'git.git': { version: '2.55.0', latestVersion: '2.55.0' } },
  );
  expect(row.updateAvailable).toBe(false);
});

test('reconcile: inventory lookup is case-insensitive on the id', () => {
  const [row] = reconcile(
    [{ formula: 'Oracle.MySQL', displayName: 'MySQL' }],
    [],
    {},
    { 'oracle.mysql': { version: '8.4.9', latestVersion: null } },
  );
  expect(row.installed).toBe(true);
});

test('reconcile: intent overrides the unlinked default', () => {
  const [row] = reconcile([{ formula: 'redis', displayName: 'Redis' }], [dto({})], {
    redis: 'linked',
  });
  expect(row.status).toBe('running');
  expect(row.linkState).toBe('linked');
});

test("reconcile: tap-qualified managed formula matches brew's bare name", () => {
  const [row] = reconcile(
    [{ formula: 'mongodb/brew/mongodb-community@7.0', displayName: 'Mongodb Community' }],
    [dto({ name: 'mongodb-community@7.0', status: 'started', healthy: true })],
    {},
  );
  expect(row.status).toBe('running');
});

test('reconcile: winget id matches its SCM service by segment', () => {
  const [row] = reconcile(
    [{ formula: 'PostgreSQL.PostgreSQL.17', displayName: 'PostgreSQL' }],
    [dto({ name: 'postgresql-x64-17', status: 'started', healthy: true })],
    {},
  );
  expect(row.status).toBe('running');
});

test('reconcile: short winget segments never match a service', () => {
  // `Git.Git` (segments of 3) must not land on any random service.
  const [row] = reconcile([{ formula: 'Git.Git', displayName: 'Git' }], [dto({})], {});
  expect(row.status).toBe('notInstalled');
});
