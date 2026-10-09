export type ServiceStatus =
  'running' | 'starting' | 'stopping' | 'stopped' | 'notInstalled' | 'installed' | 'error';

export interface StatusChipProps {
  status: ServiceStatus;
}
