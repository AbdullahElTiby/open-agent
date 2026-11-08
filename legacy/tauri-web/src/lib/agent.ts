import { Channel, invoke } from "@tauri-apps/api/core";
import type { AgentEvent, Mode, Provider, ProviderStatus } from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface RunOptions {
  runId: string;
  provider: Provider;
  prompt: string;
  cwd?: string;
  model?: string;
  mode: Mode;
  resumeId?: string;
  effort?: string;
}

export async function startRun(opts: RunOptions, onEvent: (e: AgentEvent) => void) {
  if (!isTauri) return demoRun(opts, onEvent);
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  await invoke("start_run", { req: opts, onEvent: channel });
}

export async function stopRun(runId: string) {
  if (!isTauri) {
    demoStops.add(runId);
    return;
  }
  await invoke("stop_run", { runId });
}

export async function checkProviders(): Promise<Record<Provider, ProviderStatus>> {
  if (!isTauri) {
    return {
      claude: { installed: true, version: "preview", auth: null },
      codex: { installed: true, version: "preview", auth: "Logged in using ChatGPT" },
    };
  }
  return invoke("check_providers");
}

// ── Browser preview: a scripted run so the UI can be explored without Tauri ──
const demoStops = new Set<string>();
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function demoRun(opts: RunOptions, emit: (e: AgentEvent) => void) {
  const stopped = () => demoStops.has(opts.runId);
  emit({ kind: "session", sessionId: "demo-" + opts.runId });
  emit({ kind: "thinking_start" });
  for (const w of "Let me look at the project layout first, then make the change.".split(" ")) {
    if (stopped()) return emit({ kind: "exit", code: 1 });
    emit({ kind: "thinking_delta", text: w + " " });
    await wait(40);
  }
  emit({ kind: "tool_start", id: "t1", name: opts.provider === "claude" ? "Glob" : "shell", input: opts.provider === "claude" ? { pattern: "src/**/*.ts" } : { command: "ls src" } });
  await wait(700);
  emit({ kind: "tool_end", id: "t1", output: "src/main.ts\nsrc/app.ts\nsrc/lib/util.ts", isError: false });
  emit({ kind: "todo", items: [
    { text: "Inspect project layout", done: true, active: false },
    { text: "Patch util.ts", done: false, active: true },
    { text: "Run the tests", done: false, active: false },
  ] });
  emit({ kind: "tool_start", id: "t2", name: "Edit", input: { file_path: "src/lib/util.ts", old_string: "export const sum = (a, b) => a - b;", new_string: "export const sum = (a: number, b: number) => a + b;" } });
  await wait(800);
  emit({ kind: "tool_end", id: "t2", output: "The file src/lib/util.ts has been updated.", isError: false });
  emit({ kind: "text_start" });
  const reply = "Fixed it. `sum` was **subtracting** instead of adding:\n\n```ts\nexport const sum = (a: number, b: number) => a + b;\n```\n\nThis is the browser preview — run `npm run app` to talk to the real agents.";
  for (let i = 0; i < reply.length; i += 4) {
    if (stopped()) return emit({ kind: "exit", code: 1 });
    emit({ kind: "text_delta", text: reply.slice(i, i + 4) });
    await wait(16);
  }
  emit({ kind: "text_final", text: reply });
  emit({ kind: "done", isError: false, inputTokens: 18234, outputTokens: 412, costUsd: opts.provider === "claude" ? 0.0213 : undefined, durationMs: 4200 });
  emit({ kind: "exit", code: 0 });
}
