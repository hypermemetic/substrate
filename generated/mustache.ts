// Auto-generated namespace interface
import type { /* types */ } from './types';

export interface MustacheClient {
  /** Delete a template */
  deleteTemplate(method: string, name: string, pluginId: string): Promise<MustacheEvent>;
  /** Get a specific template */
  getTemplate(method: string, name: string, pluginId: string): Promise<MustacheEvent>;
  /** List all templates for a plugin */
  listTemplates(pluginId: string): Promise<MustacheEvent>;
  /** Register a template for a plugin/method  Templates are identified by (plugin_id, method, name). If a template with the same identifier already exists, it will be updated. */
  registerTemplate(method: string, name: string, pluginId: string, template: string): Promise<MustacheEvent>;
  /** Render a value using a template  Looks up the template for the given plugin/method/name combination and renders the value using mustache templating. If template_name is None, uses "default". */
  render(method: string, pluginId: string, templateName?: string | null, value: unknown): Promise<MustacheEvent>;
}