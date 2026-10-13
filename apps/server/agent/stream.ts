// Explicitly loaded by the server; emit only display text and tool activity.
import { randomUUID } from "node:crypto";
import { appendFileSync } from "node:fs";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  let message = "";
  // Keep hook output separate from RPC's own buffered stdout. The server tails
  // this private, per-run file while Pi runs and removes it after draining it.
  const emit = (event: object) => appendFileSync(process.env.NOTEMAKER_EVENT_FILE!, JSON.stringify(event) + "\n");
  const text = (content: any) => (Array.isArray(content) ? content : [])
    .filter((part: any) => part.type === "text").map((part: any) => part.text).join("\n");
  const bounded = (value: any) => JSON.stringify(value ?? {}).slice(0, 32000);
  pi.on("session_start", async () => { emit({ kind: "ready" }); });
  pi.on("message_start", async event => {
    if (event.message.role === "assistant") {
      message = randomUUID();
      emit({ kind: "assistant", id: message, text: "" });
    }
  });
  pi.on("message_update", async event => {
    if (event.assistantMessageEvent.type === "text_delta")
      emit({ kind: "delta", id: message, text: event.assistantMessageEvent.delta });
  });
  pi.on("message_end", async event => {
    if (event.message.role === "assistant") {
      emit({ kind: "assistant_end", id: message, text: text(event.message.content) });
      if (event.message.stopReason === "error")
        emit({ kind: "error", text: event.message.errorMessage || "Provider request failed" });
    }
  });
  pi.on("tool_execution_start", async event => {
    emit({ kind: "tool", id: event.toolCallId, name: event.toolName, args: bounded(event.args), status: "running", text: "" });
  });
  pi.on("tool_execution_update", async event => {
    emit({ kind: "tool_update", id: event.toolCallId, text: text(event.partialResult?.content).slice(0, 32000) });
  });
  pi.on("tool_execution_end", async event => {
    emit({ kind: "tool_end", id: event.toolCallId, text: text(event.result?.content).slice(0, 32000), status: event.isError ? "error" : "done" });
  });
}
