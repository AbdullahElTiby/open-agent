import { useEffect, useRef, useState } from "react";
import { AlertTriangle, ArrowDown, Brain, ChevronRight, Info, Repeat2, Square } from "lucide-react";
import { useStore } from "../lib/store";
import type { Message, Part } from "../lib/types";
import { Markdown } from "./Markdown";
import { ToolCard } from "./ToolCard";
import { Orb } from "./Orb";
import { EmptyState } from "./EmptyState";

function Thinking({ p }: { p: Extract<Part, { type: "thinking" }> }) {
  const [open, setOpen] = useState(false);
  if (!p.text.trim() && !p.open) return null;
  return (
    <div className={`thinking ${p.open ? "live" : ""} ${open ? "open" : ""}`}>
      <button onClick={() => setOpen(!open)}>
        <Brain size={13} />
        <span className="thinking-label">{p.open ? "Thinking" : "Thought process"}</span>
        <ChevronRight size={13} className="tool-chev" />
      </button>
      {open && <div className="thinking-body">{p.text}</div>}
    </div>
  );
}

function Todo({ p }: { p: Extract<Part, { type: "todo" }> }) {
  const done = p.items.filter((i) => i.done).length;
  return (
    <div className="todo">
      <div className="todo-head">
        <span>Plan</span>
        <span className="todo-count">{done}/{p.items.length}</span>
        <div className="todo-bar"><i style={{ width: `${(done / Math.max(1, p.items.length)) * 100}%` }} /></div>
      </div>
      {p.items.map((it, k) => (
        <div key={k} className={`todo-item ${it.done ? "done" : ""} ${it.active ? "active" : ""}`}>
          <span className="todo-check" />
          <span>{it.text}</span>
        </div>
      ))}
    </div>
  );
}

function Notice({ p }: { p: Extract<Part, { type: "notice" }> }) {
  const Icon = p.tone === "error" ? AlertTriangle : p.tone === "relay" ? Repeat2 : Info;
  return (
    <div className={`notice ${p.tone}`}>
      <Icon size={14} />
      <span>{p.text}</span>
    </div>
  );
}

function fmtTokens(n?: number) {
  if (n == null) return "";
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : String(n);
}

function AgentMessage({ m }: { m: Message }) {
  const streaming = m.status === "streaming";
  const visible = m.parts.filter((p) => !(p.type === "tool" && p.name === "__hidden"));
  const hasText = visible.some((p) => p.type === "text" && p.text);
  return (
    <div className={`msg agent ${m.provider} ${streaming ? "streaming" : ""}`}>
      <div className="msg-avatar">
        <Orb size={26} small />
      </div>
      <div className="msg-body">
        <div className="msg-who">
          <span className="who-name">{m.provider === "claude" ? "Claude" : "Codex"}</span>
          {m.meta?.model && <span className="who-model">{m.meta.model}</span>}
        </div>
        {visible.map((p, k) => {
          switch (p.type) {
            case "text": return p.text ? <div key={k} className={`text ${p.open ? "typing" : ""}`}><Markdown text={p.text} /></div> : null;
            case "thinking": return <Thinking key={k} p={p} />;
            case "tool": return <ToolCard key={(p as any).id ?? k} t={p} />;
            case "todo": return <Todo key={k} p={p} />;
            case "notice": return <Notice key={k} p={p} />;
          }
        })}
        {streaming && !hasText && visible.every((p) => p.type !== "thinking" || !p.open) && (
          <div className="working"><span /><span /><span /></div>
        )}
        {m.status === "stopped" && <div className="notice info"><Square size={12} /> <span>Stopped</span></div>}
        {!streaming && m.meta && (
          <div className="msg-meta">
            {m.meta.durationMs != null && <span>{(m.meta.durationMs / 1000).toFixed(1)}s</span>}
            {m.meta.inputTokens != null && <span>↑ {fmtTokens(m.meta.inputTokens)}</span>}
            {m.meta.outputTokens != null && <span>↓ {fmtTokens(m.meta.outputTokens)}</span>}
          </div>
        )}
      </div>
    </div>
  );
}

function UserMessage({ m }: { m: Message }) {
  const text = m.parts.map((p) => (p.type === "text" ? p.text : "")).join("");
  return (
    <div className={`msg user ${m.provider}`}>
      <div className="bubble">{text}</div>
    </div>
  );
}

export function Conversation() {
  const session = useStore((s) => s.active());
  const ref = useRef<HTMLDivElement>(null);
  const [pinned, setPinned] = useState(true);

  const last = session.messages[session.messages.length - 1];
  const sig = last ? JSON.stringify(last.parts).length + session.messages.length : 0;

  useEffect(() => {
    if (pinned && ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [sig, pinned, session.id]);

  useEffect(() => setPinned(true), [session.id]);

  if (session.messages.length === 0) return <EmptyState />;

  return (
    <div
      className="conversation"
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        setPinned(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
      }}
    >
      <div className="conv-inner">
        {session.messages.map((m) =>
          m.role === "user" ? <UserMessage key={m.id} m={m} /> : <AgentMessage key={m.id} m={m} />,
        )}
      </div>
      {!pinned && (
        <button className="jump" onClick={() => setPinned(true)}>
          <ArrowDown size={14} /> Latest
        </button>
      )}
    </div>
  );
}
