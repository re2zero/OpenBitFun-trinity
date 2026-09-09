/**
 * Trinity cognitive engine API.
 *
 * Thin wrappers over the `trinity_*` Tauri commands. Every call forwards to
 * the `trinityd` daemon via the desktop host; the daemon owns all cognitive
 * state. All methods degrade to a thrown error when the daemon is offline.
 */

import { api } from './ApiClient';
import { createTauriCommandError } from '../errors/TauriCommandError';

export class TrinityAPI {
  private async invoke(command: string, params: Record<string, unknown> = {}): Promise<any> {
    try {
      return await api.invoke(command, { params });
    } catch (error) {
      throw createTauriCommandError(command, error, params);
    }
  }

  // ── Cognitive state ────────────────────────────────────────────

  async getCognitiveState(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_get_cognitive_state', params);
  }

  async express(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_express', params);
  }

  async selfPerception(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_self_perception', params);
  }

  async getStatus(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_get_status', params);
  }

  async cognitionHistory(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_cognition_history', params);
  }

  // ── Memory (MindGraph) ─────────────────────────────────────────

  async memoryTimeline(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_memory_timeline', params);
  }

  async memoryStats(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_memory_stats', params);
  }

  async recallMemory(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_recall_memory', params);
  }

  async memorize(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_memorize', params);
  }

  async forgetMemory(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_forget_memory', params);
  }

  async reinforceMemory(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_reinforce_memory', params);
  }

  // ── Awakening ceremony ─────────────────────────────────────────

  async awaken(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_awaken', params);
  }

  // ── Cognitive engine LLM config ────────────────────────────────

  async llmGetConfig(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_llm_get_config', params);
  }

  async llmSetConfig(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_llm_set_config', params);
  }

  async llmTestConnection(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_llm_test_connection', params);
  }

  // ── Shutdown ───────────────────────────────────────────────────

  async shutdown(params: Record<string, unknown> = {}): Promise<any> {
    return this.invoke('trinity_shutdown', params);
  }
}

export const trinityAPI = new TrinityAPI();