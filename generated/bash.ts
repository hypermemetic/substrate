// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface BashClient {
  /** Execute a bash command and stream stdout, stderr, and exit code */
  execute(command: string): AsyncGenerator<BashEvent>;
}