import { Activity, Coins, FileCode2, Layers, Timer } from "lucide-react";
import { toolTarget, useStore } from "../lib/store";
import type { Part } from "../lib/types";

type Tool = Extract<Part, { type: "tool" }>;

function fmt(n: number) {
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "k";
  return String(n);
}

export function ActivityRail() {
  const session = useStore((s) => s.active());
  const running = useStore((s) => !!s.runs[s.activeId]);

  const agentMsgs = session.messages.filter((m) => m.role === "agent");
  const tools: Tool[] = agentMsgs.flatMap((m) => m.parts.filter((p): p is Tool => p.type === "tool" && p.name !== "__hidden"));
  const tokensIn = agentMsgs.reduce((a, m) => a + (m.meta?.inputTokens ?? 0), 0);
  const tokensOut = agentMsgs.reduce((a, m) => a + (m.meta?.outputTokens ?? 0), 0);
  const cost = agentMsgs.reduce((a, m) => a + (m.meta?.costUsd ?? 0), 0);
  const time = agentMsgs.reduce((a, m) => a + (m.meta?.durationMs ?? 0), 0);

  const files = new Map<string, string>();
  for (const t of tools) {
    if (["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(t.name) && t.input?.file_path) files.set(t.input.file_path, t.name === "Write" ? "add" : "mod");
    if (t.name === "file_change") for (const c of t.input?.changes ?? []) files.set(c.path, c.kind === "add" ? "add" : c.kind === "delete" ? "del" : "mod");
  }

  const recent = tools.slice(-14).reverse();

  return (
    <aside className="rail glass">
      <div className="rail-section">
        <div className="rail-title"><Activity size={13} /> Session pulse</div>
        <div className={`pulse ${running ? "live" : ""}`}>
          <svg viewBox="0 0 200 40" preserveAspectRatio="none">
            <path d="M0 20 L40 20 L50 8 L60 32 L70 14 L78 24 L86 20 L200 20" />
          </svg>
        </div>
        <div className="stats">
          <div className="stat"><Layers size={13} /><b>{agentMsgs.length}</b><span>turns</span></div>
          <div className="stat"><Timer size={13} /><b>{(time / 1000).toFixed(0)}s</b><span>agent time</span></div>
          <div className="stat"><span className="arrow">↑</span><b>{fmt(tokensIn)}</b><span>tokens in</span></div>
          <div className="stat"><span className="arrow">↓</span><b>{fmt(tokensOut)}</b><span>tokens out</span></div>
        </div>
        {cost > 0 && (
          <div className="cost" title="What this would cost on the API. Your subscription covers it.">
            <Coins size={13} /> <span>API-equivalent</span> <b>${cost.toFixed(3)}</b>
          </div>
        )}
      </div>

      <div className="rail-section">
        <div className="rail-title"><FileCode2 size={13} /> Files touched <span className="count">{files.size}</span></div>
        {files.size === 0 ? (
          <div className="rail-empty">No edits yet</div>
        ) : (
          <div className="files">
            {[...files.entries()].map(([f, k]) => (
              <div key={f} className="file" title={f}>
                <span className={`fk ${k}`}>{k === "add" ? "A" : k === "del" ? "D" : "M"}</span>
                <span className="fname">{f.split(/[\\/]/).pop()}</span>
                <span className="fdir">{f.split(/[\\/]/).slice(-2, -1)[0]}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="rail-section grow">
        <div className="rail-title"><Activity size={13} /> Timeline <span className="count">{tools.length}</span></div>
        {recent.length === 0 ? (
          <div className="rail-empty">Tool calls will stream here</div>
        ) : (
          <div className="timeline">
            {recent.map((t) => (
              <div key={t.id} className={`tl ${t.status}`}>
                <span className="tl-dot" />
                <span className="tl-name">{t.name === "file_change" ? "patch" : t.name}</span>
                <span className="tl-target">{String(toolTarget(t.input) || (t.input?.changes?.[0]?.path ?? "")).split(/[\\/]/).pop()}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="threads">
        {(["claude", "codex"] as const).map((p) => (
          <div key={p} className={`thread ${session.threads[p] ? "on" : ""}`}>
            <span className={`p-dot ${p} ${session.threads[p] ? "on" : ""}`} />
            <span>{p === "claude" ? "Claude" : "Codex"} thread</span>
            <code>{session.threads[p]?.slice(0, 8) ?? "—"}</code>
          </div>
        ))}
      </div>
    </aside>
  );
}
