// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface EchoClient {
  /** Echo a message back */
  echo(count: number, message: string): Promise<EchoEvent>;
  /** Echo a simple message once */
  once(message: string): Promise<EchoEvent>;
}