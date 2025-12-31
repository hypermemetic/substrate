// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface SolarClient {
  /** Get information about a specific celestial body */
  info(path: string): Promise<SolarEvent>;
  /** Observe the entire solar system */
  observe(): Promise<SolarEvent>;
}