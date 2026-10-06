import type { Message } from '@earendil-works/pi-ai';

// One user-authorized start, then only continuations of receipted tool calls.
// Errors, aborts, length recovery, compaction and idle warming cannot authorize
// another paid invocation. Deliberately submitting another prompt can.
export class ReplayGuard {
  private ready = false;
  private continuation: { model: string; calls: readonly { id: string; name: string }[] } | undefined;

  beginRun(): void {
    this.ready = true;
    this.continuation = undefined;
  }

  close(): void {
    this.ready = false;
    this.continuation = undefined;
  }

  claim(model: string, messages: readonly Message[]): void {
    if (this.ready) {
      this.close();
      return;
    }
    const previous = this.continuation;
    this.close();
    if (!previous || previous.model !== model) throw new Error('possums_automatic_replay_blocked');
    const lastAssistant = messages.findLastIndex(message => message.role === 'assistant');
    const assistant = messages[lastAssistant];
    if (assistant?.role !== 'assistant' || assistant.provider !== 'possums' || assistant.model !== model || assistant.stopReason !== 'toolUse') {
      throw new Error('possums_automatic_replay_blocked');
    }
    const calls = assistant.content.filter(block => block.type === 'toolCall');
    const expected = new Map(previous.calls.map(call => [call.id, call.name]));
    if (calls.length !== expected.size || new Set(calls.map(call => call.id)).size !== expected.size || calls.some(call => expected.get(call.id) !== call.name)) {
      throw new Error('possums_automatic_replay_blocked');
    }
    const received = new Set<string>();
    for (const message of messages.slice(lastAssistant + 1)) {
      if (message.role === 'system') continue;
      if (message.role !== 'toolResult' || expected.get(message.toolCallId) !== message.toolName || received.has(message.toolCallId)) {
        throw new Error('possums_automatic_replay_blocked');
      }
      received.add(message.toolCallId);
    }
    if (received.size !== expected.size) throw new Error('possums_automatic_replay_blocked');
  }

  complete(model: string, reason: 'stop' | 'length' | 'tool_calls', calls: readonly { id: string; name: string }[]): void {
    this.close();
    if (reason !== 'tool_calls') return;
    if (!calls.length || new Set(calls.map(call => call.id)).size !== calls.length) {
      throw new Error('possums_invalid_tool_receipt');
    }
    this.continuation = { model, calls: calls.map(call => ({ id: call.id, name: call.name })) };
  }
}
