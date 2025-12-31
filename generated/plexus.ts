// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface PlexusClient {
  /** Route a call to a registered activation */
  call(method: string, params?: unknown): AsyncGenerator<CallEvent>;
  /** Get plexus configuration hash (from the recursive schema)  This hash changes whenever any method or child plugin changes. It's computed from the method hashes rolled up through the schema tree. */
  hash(): Promise<HashEvent>;
}