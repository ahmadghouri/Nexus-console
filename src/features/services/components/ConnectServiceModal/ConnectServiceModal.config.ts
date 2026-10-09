/** Quick-add suggestions (just UX shortcuts — search finds any formula).
 * One list per package manager: winget ids are what Windows installs under;
 * brew formula names otherwise. Every Windows id below was verified against
 * the live winget sources (`winget show --id … --exact`) — a bad id now fails
 * the install availability gate with "This package is not available in WinGet"
 * (see winops::install_formula). RabbitMQ's Windows server isn't published to
 * winget (only GUI clients are), so it is deliberately absent. */
export const POPULAR_FORMULAE = [
  'mysql',
  'mariadb',
  'valkey',
  'mongodb-community',
  'memcached',
  'rabbitmq',
  'nginx',
  'elasticsearch',
];

export const POPULAR_WINGET_IDS = [
  'PostgreSQL.PostgreSQL.17',
  'Oracle.MySQL',
  'MariaDB.Server',
  'Redis.Redis',
  'nginxinc.nginx',
];
