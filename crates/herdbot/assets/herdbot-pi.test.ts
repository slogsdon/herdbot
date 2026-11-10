import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
import { test } from "node:test";

test("agent_end forwards only the final assistant's text and error metadata", async (context) => {
  let payload: Record<string, unknown> = {};
  context.mock.method(childProcess, "execFile", () => ({
    stdin: { end: (input: string) => { payload = JSON.parse(input); } },
  }));
  syncBuiltinESMExports();
  const { default: register } = await import("./herdbot-pi.ts");
  const handlers = new Map<string, (event: { messages: unknown[] }) => void>();
  register({
    on: (event: string, handler: (event: { messages: unknown[] }) => void) => handlers.set(event, handler),
    registerMcpServer() {},
  } as unknown as Parameters<typeof register>[0]);
  const agentEnd = handlers.get("agent_end");
  assert.ok(agentEnd);
  const emit = (messages: unknown[]) => agentEnd({ messages });
  emit([
    { role: "assistant", content: [{ type: "text", text: "stale success" }] },
    { role: "assistant", content: [], stopReason: "error", errorMessage: "Request timed out" },
  ]);
  assert.deepEqual(payload, { event: "agent_end", text: "", stopReason: "error", errorMessage: "Request timed out" });
  emit([{ role: "assistant", content: [{ type: "thinking", thinking: "private" }, { type: "text", text: "done" }], stopReason: "stop" }]);
  assert.deepEqual(payload, { event: "agent_end", text: "done", stopReason: "stop" });
  emit([{ role: "assistant", content: "legacy" }]);
  assert.deepEqual(payload, { event: "agent_end", text: "legacy" });
  emit([{ role: "assistant", content: "stale" }, { role: "assistant", stopReason: "error" }]);
  assert.deepEqual(payload, { event: "agent_end", text: "", stopReason: "error" });
  emit([]);
  assert.deepEqual(payload, { event: "agent_end", text: "" });
  context.mock.restoreAll();
  syncBuiltinESMExports();
});
