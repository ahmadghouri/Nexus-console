import { z } from 'zod';

/** Mirrors Rust `ServiceDto` (serde camelCase). null when unknown/not running.
 * displayName/startType/noteworthy are Windows SCM metadata, skipped from the
 * macOS payload — hence optional. */
export const serviceDtoSchema = z.object({
  name: z.string(),
  status: z.string(),
  pid: z.number().nullable(),
  cpu: z.number().nullable(),
  memoryBytes: z.number().nullable(),
  uptimeSecs: z.number().nullable(),
  port: z.number().nullable(),
  healthy: z.boolean().nullable(),
  plist: z.string().nullable(),
  displayName: z.string().optional(),
  startType: z.string().optional(),
  noteworthy: z.boolean().optional(),
});
export const servicesSchema = z.array(serviceDtoSchema);
export type ServiceDto = z.infer<typeof serviceDtoSchema>;
