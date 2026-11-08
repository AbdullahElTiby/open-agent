import { create } from "zustand";
import { persist } from "zustand/middleware";
import { startRun, stopRun } from "./agent";
import type { AgentEvent, Message, Mode, Part, Provider, ProviderStatus, Session } from "./types";

const uid = () => Math.random().toString(36).slice(2, 10) + Date.now().toString(36).slice(-4);

function newSession(prev?: Session): Session {
  return {
    id: uid(),
    title: "New session",
    cwd: prev?.cwd ?? "",
    provider: prev?.provider ?? "claude",
    mode: prev?.mode ?? "build",
    models: prev?.models ?? { claude: "default", codex: "default" },
    effort: prev?.effort ?? { claude: "default", codex: "default" },
    threads: {},
    messages: [],
    createdAt: Date.now(),
    updatedAt: Date.now(),
  };
}

interface State {
  sessions: Session[];
  activeId: string;
  runs: Record<string, string>; // sessionId -> runId
  providers: Partial<Record<Provider, ProviderStatus>>;
  railOpen: boolean;
  sidebarOpen: boolean;

  active(): Session;
  setProviders(p: Record<Provider, ProviderStatus>): void;
  newSession(): void;
  selectSession(id: string): void;
  deleteSession(id: string): void;
  update(patch: Partial<Session>): void;
  send(prompt: string): Promise<void>;
  stop(): void;
  toggleRail(): void;
  toggleSidebar(): void;
}

/** Build a context hand-off when the session switches to a provider with no thread yet. */
function relayContext(s: Session, to: Provider): string | null {
  if (s.threads[to] || s.messages.length === 0) return null;
  const lines: string[] = [];
  for (const m of s.messages.slice(-14)) {
    const text = m.parts
      .map((p) =>
        p.type === "text" ? p.text :
        p.type === "tool" ? `[${p.name}${toolTarget(p.input) ? " " + toolTarget(p.input) : ""}]` : "",
      )
      .filter(Boolean)
      .join("\n")
      .trim();
    if (text) lines.push(`${m.role === "user" ? "USER" : `ASSISTANT (${m.provider})`}: ${text}`);
  }
  let ctx = lines.join("\n\n");
  if (ctx.length > 8000) ctx = "…" + ctx.slice(-8000);
  return `<previous_conversation note="This session was handed over to you from another coding agent working in the same workspace. Continue from here.">\n${ctx}\n</previous_conversation>\n\n`;
}

export function toolTarget(input: any): string {
  if (!input || typeof input !== "object") return "";
  return (
    input.file_path ?? input.path ?? input.pattern ?? input.command ?? input.query ?? input.url ?? input.description ?? ""
  ).toString();
}

function applyEvent(msg: Message, e: AgentEvent): Message {
  const parts = [...msg.parts];
  const last = parts[parts.length - 1];
  const closeOpen = () => {
    const l = parts[parts.length - 1];
    if (l && (l.type === "text" || l.type === "thinking") && l.open) parts[parts.length - 1] = { ...l, open: false };
  };

  switch (e.kind) {
    case "text_start":
      closeOpen();
      parts.push({ type: "text", text: "", open: true });
      break;
    case "text_delta":
      if (last?.type === "text" && last.open) parts[parts.length - 1] = { ...last, text: last.text + e.text };
      else { closeOpen(); parts.push({ type: "text", text: e.text, open: true }); }
      break;
    case "text_final":
      if (last?.type === "text" && last.open) parts[parts.length - 1] = { type: "text", text: e.text };
      else if (e.text?.trim()) { closeOpen(); parts.push({ type: "text", text: e.text }); }
      break;
    case "thinking_start":
      closeOpen();
      parts.push({ type: "thinking", text: "", open: true });
      break;
    case "thinking_delta":
      if (last?.type === "thinking" && last.open) parts[parts.length - 1] = { ...last, text: last.text + e.text };
      else { closeOpen(); parts.push({ type: "thinking", text: e.text, open: true }); }
      break;
    case "thinking_final":
      if (last?.type === "thinking" && last.open) parts[parts.length - 1] = { type: "thinking", text: e.text || last.text };
      else if (e.text?.trim()) { closeOpen(); parts.push({ type: "thinking", text: e.text }); }
      break;
    case "tool_start": {
      if (parts.some((p) => p.type === "tool" && p.id === e.id)) break;
      closeOpen();
      // Claude's TodoWrite becomes a live checklist instead of a tool card.
      if (e.name === "TodoWrite" && Array.isArray(e.input?.todos)) {
        const items = e.input.todos.map((t: any) => ({
          text: t.status === "in_progress" && t.activeForm ? t.activeForm : t.content,
          done: t.status === "completed",
          active: t.status === "in_progress",
        }));
        const i = parts.findIndex((p) => p.type === "todo");
        if (i >= 0) parts.splice(i, 1);
        parts.push({ type: "todo", items });
        parts.push({ type: "tool", id: e.id, name: "__hidden", input: null, status: "running" });
        break;
      }
      parts.push({ type: "tool", id: e.id, name: e.name, input: e.input, status: "running" });
      break;
    }
    case "tool_update": {
      const i = parts.findIndex((p) => p.type === "tool" && p.id === e.id);
      if (i >= 0) parts[i] = { ...(parts[i] as any), output: e.output };
      break;
    }
    case "tool_end": {
      const i = parts.findIndex((p) => p.type === "tool" && p.id === e.id);
      if (i >= 0) parts[i] = { ...(parts[i] as any), output: e.output, status: e.isError ? "error" : "ok" };
      break;
    }
    case "todo": {
      const i = parts.findIndex((p) => p.type === "todo");
      if (i >= 0) parts[i] = { type: "todo", items: e.items };
      else parts.push({ type: "todo", items: e.items });
      break;
    }
    case "notice":
      if (/hook/i.test(e.text)) break; // local hook chatter
      parts.push({ type: "notice", text: e.text, tone: "info" });
      break;
    case "error": {
      const dup = parts.some((p) => p.type === "notice" && p.tone === "error" && p.text === e.message);
      if (!dup && e.message) { closeOpen(); parts.push({ type: "notice", text: e.message, tone: "error" }); }
      return { ...msg, parts, status: "error" };
    }
    case "done":
      closeOpen();
      return {
        ...msg,
        parts: parts.map((p) => (p.type === "tool" && p.status === "running" ? { ...p, status: "ok" } : p)),
        status: e.isError || msg.status === "error" ? "error" : "done",
        meta: {
          ...msg.meta,
          costUsd: e.costUsd ?? undefined,
          inputTokens: e.inputTokens ?? undefined,
          outputTokens: e.outputTokens ?? undefined,
          durationMs: e.durationMs ?? Date.now() - msg.createdAt,
        },
      };
    case "session":
      if (e.model) return { ...msg, meta: { ...msg.meta, model: e.model } };
      return msg;
    case "exit":
      closeOpen();
      return {
        ...msg,
        parts: parts.map((p) => (p.type === "tool" && p.status === "running" ? { ...p, status: msg.status === "stopped" ? "error" : "ok" } : p)),
        status: msg.status === "streaming" ? (e.code === 0 ? "done" : "error") : msg.status,
      };
  }
  return { ...msg, parts };
}

export const useStore = create<State>()(
  persist(
    (set, get) => {
      const first = newSession();
      const patchSession = (id: string, fn: (s: Session) => Session) =>
        set((st) => ({ sessions: st.sessions.map((s) => (s.id === id ? fn(s) : s)) }));

      return {
        sessions: [first],
        activeId: first.id,
        runs: {},
        providers: {},
        railOpen: true,
        sidebarOpen: true,

        active() {
          const st = get();
          return st.sessions.find((s) => s.id === st.activeId) ?? st.sessions[0];
        },
        setProviders(p) {
          set({ providers: p });
        },
        newSession() {
          const cur = get().active();
          if (cur && cur.messages.length === 0) return; // reuse the empty one
          const s = newSession(cur);
          set((st) => ({ sessions: [s, ...st.sessions], activeId: s.id }));
        },
        selectSession(id) {
          set({ activeId: id });
        },
        deleteSession(id) {
          const runId = get().runs[id];
          if (runId) stopRun(runId);
          set((st) => {
            let sessions = st.sessions.filter((s) => s.id !== id);
            if (sessions.length === 0) sessions = [newSession(st.sessions[0])];
            return {
              sessions,
              activeId: st.activeId === id ? sessions[0].id : st.activeId,
            };
          });
        },
        update(patch) {
          const id = get().activeId;
          patchSession(id, (s) => ({ ...s, ...patch, updatedAt: Date.now() }));
        },
        toggleRail() {
          set((st) => ({ railOpen: !st.railOpen }));
        },
        toggleSidebar() {
          set((st) => ({ sidebarOpen: !st.sidebarOpen }));
        },

        async send(prompt) {
          const s = get().active();
          if (!prompt.trim() || get().runs[s.id]) return;
          const provider = s.provider;
          const relay = relayContext(s, provider);
          const runId = uid();

          const userMsg: Message = {
            id: uid(), role: "user", provider, createdAt: Date.now(),
            parts: [{ type: "text", text: prompt }],
          };
          const agentMsg: Message = {
            id: uid(), role: "agent", provider, createdAt: Date.now(), status: "streaming",
            parts: relay
              ? [{ type: "notice", tone: "relay", text: `Context relayed to ${provider === "claude" ? "Claude" : "Codex"}` }]
              : [],
          };
          const title = s.messages.length === 0 ? prompt.trim().replace(/\s+/g, " ").slice(0, 60) : s.title;

          patchSession(s.id, (x) => ({
            ...x, title, updatedAt: Date.now(),
            messages: [...x.messages, userMsg, agentMsg],
          }));
          set((st) => ({ runs: { ...st.runs, [s.id]: runId } }));

          const onEvent = (e: AgentEvent) => {
            patchSession(s.id, (x) => {
              const threads = e.kind === "session" || (e.kind === "done" && e.sessionId)
                ? { ...x.threads, [provider]: (e as any).sessionId } : x.threads;
              return {
                ...x, threads, updatedAt: Date.now(),
                messages: x.messages.map((m) => (m.id === agentMsg.id ? applyEvent(m, e) : m)),
              };
            });
            if (e.kind === "exit") {
              set((st) => {
                const runs = { ...st.runs };
                delete runs[s.id];
                return { runs };
              });
            }
          };

          try {
            await startRun(
              {
                runId, provider,
                prompt: (relay ?? "") + prompt,
                cwd: s.cwd || undefined,
                model: s.models[provider],
                effort: s.effort[provider],
                mode: s.mode,
                resumeId: s.threads[provider],
              },
              onEvent,
            );
          } catch (err) {
            onEvent({ kind: "error", message: String(err) });
            onEvent({ kind: "exit", code: 1 });
          }
        },

        stop() {
          const s = get().active();
          const runId = get().runs[s.id];
          if (!runId) return;
          patchSession(s.id, (x) => ({
            ...x,
            messages: x.messages.map((m) => (m.status === "streaming" ? { ...m, status: "stopped" } : m)),
          }));
          stopRun(runId);
        },
      };
    },
    {
      name: "open-agent",
      version: 1,
      partialize: (st) => ({
        sessions: st.sessions,
        activeId: st.activeId,
        railOpen: st.railOpen,
        sidebarOpen: st.sidebarOpen,
      }),
      // Any run that was live when the app closed is over now.
      onRehydrateStorage: () => (st) => {
        if (!st) return;
        st.sessions = st.sessions.map((s) => ({
          ...s,
          messages: s.messages.map((m) => (m.status === "streaming" ? { ...m, status: "stopped" } : m)),
        }));
      },
    },
  ),
);

export type { Mode, Part };
