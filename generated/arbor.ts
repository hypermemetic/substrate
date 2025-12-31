// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface ArborClient {
  /** Get all external handles in the path to a node */
  contextGetHandles(nodeId: UUID, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Get the full path data from root to a node */
  contextGetPath(nodeId: UUID, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** List all leaf nodes in a tree */
  contextListLeaves(treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Create an external node in a tree */
  nodeCreateExternal(handle: Handle, metadata?: unknown, parent?: UUID | null, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Create a text node in a tree */
  nodeCreateText(content: string, metadata?: unknown, parent?: UUID | null, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Get a node by ID */
  nodeGet(nodeId: UUID, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Get the children of a node */
  nodeGetChildren(nodeId: UUID, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Get the parent of a node */
  nodeGetParent(nodeId: UUID, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Get the path from root to a node */
  nodeGetPath(nodeId: UUID, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Claim ownership of a tree (increment reference count) */
  treeClaim(count: number, ownerId: string, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Create a new conversation tree */
  treeCreate(metadata?: unknown, ownerId: string): AsyncGenerator<ArborEvent>;
  /** Retrieve a complete tree with all nodes */
  treeGet(treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Get lightweight tree structure without node data */
  treeGetSkeleton(treeId: UUID): AsyncGenerator<ArborEvent>;
  /** List all active trees */
  treeList(): AsyncGenerator<ArborEvent>;
  /** List archived trees */
  treeListArchived(): AsyncGenerator<ArborEvent>;
  /** List trees scheduled for deletion */
  treeListScheduled(): AsyncGenerator<ArborEvent>;
  /** Release ownership of a tree (decrement reference count) */
  treeRelease(count: number, ownerId: string, treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Render tree as text visualization */
  treeRender(treeId: UUID): AsyncGenerator<ArborEvent>;
  /** Update tree metadata */
  treeUpdateMetadata(metadata: unknown, treeId: UUID): AsyncGenerator<ArborEvent>;
}