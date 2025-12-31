// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface HealthClient {
  /** Check the health status of the hub and return uptime */
  check(): Promise<HealthEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<SchemaResult>;
}