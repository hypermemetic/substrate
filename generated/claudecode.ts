// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface ClaudecodeClient {
  /** Chat with a session, streaming tokens like Cone */
  chat(ephemeral?: boolean | null, name: string, prompt: string): AsyncGenerator<ChatEvent>;
  /** Create a new Claude Code session */
  create(model: Model, name: string, systemPrompt?: string | null, workingDir: string): Promise<CreateResult>;
  /** Delete a session */
  delete(name: string): Promise<DeleteResult>;
  /** Fork a session to create a branch point */
  fork(name: string, newName: string): Promise<ForkResult>;
  /** Get session configuration details */
  get(name: string): Promise<GetResult>;
  /** List all Claude Code sessions */
  list(): Promise<ListResult>;
}