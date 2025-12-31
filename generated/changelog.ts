// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface ChangelogClient {
  /** Add a changelog entry for a plexus hash transition */
  add(author?: string | null, details?: unknown[] | null, hash: string, previousHash?: string | null, queueId?: string | null, summary: string): AsyncGenerator<ChangelogEvent>;
  /** Check current status - is the current plexus hash documented? */
  check(currentHash: string): AsyncGenerator<ChangelogEvent>;
  /** Get a specific changelog entry by hash */
  get(hash: string): AsyncGenerator<ChangelogEvent>;
  /** List all changelog entries */
  list(): AsyncGenerator<ChangelogEvent>;
  /** Add a planned change to the queue */
  queueAdd(description: string, tags?: unknown[] | null): AsyncGenerator<ChangelogEvent>;
  /** Mark a queue entry as complete */
  queueComplete(hash: string, id: string): AsyncGenerator<ChangelogEvent>;
  /** Get a specific queue entry by ID */
  queueGet(id: string): AsyncGenerator<ChangelogEvent>;
  /** List all queue entries, optionally filtered by tag */
  queueList(tag?: string | null): AsyncGenerator<ChangelogEvent>;
  /** List pending queue entries, optionally filtered by tag */
  queuePending(tag?: string | null): AsyncGenerator<ChangelogEvent>;
}