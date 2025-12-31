// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface ConeClient {
  /** Chat with a cone - appends prompt to context, calls LLM, advances head */
  chat(ephemeral?: boolean | null, identifier: ConeIdentifier, prompt: string): AsyncGenerator<ChatEvent>;
  /** Create a new cone (LLM agent with persistent conversation context) */
  create(metadata?: unknown, modelId: string, name: string, systemPrompt?: string | null): Promise<CreateResult>;
  /** Delete a cone (associated tree is preserved) */
  delete(identifier: ConeIdentifier): Promise<DeleteResult>;
  /** Get cone configuration by name or ID */
  get(identifier: ConeIdentifier): Promise<GetResult>;
  /** List all cones */
  list(): Promise<ListResult>;
  /** Get available LLM services and models */
  registry(): Promise<RegistryResult>;
  /** Move cone's canonical head to a different node in the tree */
  setHead(identifier: ConeIdentifier, nodeId: UUID): Promise<SetHeadResult>;
}