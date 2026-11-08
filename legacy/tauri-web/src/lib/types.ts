export type Provider = "claude" | "codex";
export type Mode = "plan" | "build" | "auto";

export type ToolStatus = "running" | "ok" | "error";

export type Part =
  | { type: "text"; text: string; open?: boolean }
  | { type: "thinking"; text: string; open?: boolean }
  | { type: "tool"; id: string; name: string; input: any; output?: string; status: ToolStatus }
  | { type: "todo"; items: TodoItem[] }
  | { type: "notice"; text: string; tone: "info" | "warn" | "error" | "relay" };

export interface TodoItem {
  text: string;
  done: boolean;
  active: boolean;
}

export interface MessageMeta {
  costUsd?: number;
  inputTokens?: number;
  outputTokens?: number;
  durationMs?: number;
  model?: string;
}

export interface Message {
  id: string;
  role: "user" | "agent";
  provider: Provider;
  parts: Part[];
  createdAt: number;
  status?: "streaming" | "done" | "error" | "stopped";
  meta?: MessageMeta;
}

export interface Session {
  id: string;
  title: string;
  cwd: string;
  provider: Provider;
  mode: Mode;
  models: Record<Provider, string>;
  effort: Record<Provider, string>;
  threads: Partial<Record<Provider, string>>;
  messages: Message[];
  createdAt: number;
  updatedAt: number;
}

export interface ProviderStatus {
  installed: boolean;
  version?: string | null;
  path?: string | null;
  auth?: string | null;
  models?: string[] | null;
}

export type AgentEvent =
  | { kind: "session"; sessionId: string; model?: string }
  | { kind: "text_start" }
  | { kind: "text_delta"; text: string }
  | { kind: "text_final"; text: string }
  | { kind: "thinking_start" }
  | { kind: "thinking_delta"; text: string }
  | { kind: "thinking_final"; text: string }
  | { kind: "tool_start"; id: string; name: string; input: any }
  | { kind: "tool_update"; id: string; output: string }
  | { kind: "tool_end"; id: string; output: string; isError: boolean }
  | { kind: "todo"; items: TodoItem[] }
  | { kind: "notice"; text: string }
  | { kind: "error"; message: string }
  | {
      kind: "done";
      sessionId?: string;
      costUsd?: number;
      inputTokens?: number;
      outputTokens?: number;
      durationMs?: number;
      isError: boolean;
    }
  | { kind: "exit"; code: number | null };
