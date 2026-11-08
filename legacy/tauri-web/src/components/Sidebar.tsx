import { Plus, Trash2 } from "lucide-react";
import { useStore } from "../lib/store";
import type { Provider, Session } from "../lib/types";

function groupOf(ts: number) {
  const d = new Date(ts);
  const now = new Date();
  const day = 86400000;
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  if (ts >= startToday) return "Today";
  if (ts >= startToday - day) return "Yesterday";
  if (ts >= startToday - 7 * day) return "This week";
  return d.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

export function Sidebar() {
  const sessions = useStore((s) => s.sessions);
  const activeId = useStore((s) => s.activeId);
  const runs = useStore((s) => s.runs);
  const providers = useStore((s) => s.providers);
  const { newSession, selectSession, deleteSession } = useStore.getState();

  const sorted = [...sessions].sort((a, b) => b.updatedAt - a.updatedAt);
  const groups = new Map<string, Session[]>();
  for (const s of sorted) {
    const g = groupOf(s.updatedAt);
    groups.set(g, [...(groups.get(g) ?? []), s]);
  }

  return (
    <aside className="sidebar glass">
      <div className="brand">
        <div className="brand-mark">
          <span />
        </div>
        <div className="brand-text">
          <div className="brand-name">Open Agent</div>
          <div className="brand-sub">Claude × Codex</div>
        </div>
      </div>

      <button className="new-btn" onClick={newSession} title="New session (Ctrl+N)">
        <Plus size={16} />
        <span>New session</span>
        <kbd>Ctrl N</kbd>
      </button>

      <nav className="session-list">
        {[...groups.entries()].map(([g, list]) => (
          <div key={g} className="session-group">
            <div className="group-label">{g}</div>
            {list.map((s) => (
              <div
                key={s.id}
                className={`session-item ${s.id === activeId ? "active" : ""}`}
                onClick={() => selectSession(s.id)}
              >
                <span className={`p-dot ${s.provider} ${runs[s.id] ? "live" : ""}`} />
                <span className="session-title">{s.title}</span>
                <button
                  className="session-del"
                  title="Delete session"
                  onClick={(e) => {
                    e.stopPropagation();
                    deleteSession(s.id);
                  }}
                >
                  <Trash2 size={13} />
                </button>
              </div>
            ))}
          </div>
        ))}
      </nav>

      <div className="provider-cards">
        {(["claude", "codex"] as Provider[]).map((p) => {
          const st = providers[p];
          const ok = st?.installed;
          return (
            <div key={p} className={`provider-card ${p} ${ok ? "ok" : st ? "missing" : ""}`}>
              <span className={`p-dot ${p} ${ok ? "on" : ""}`} />
              <div className="pc-text">
                <div className="pc-name">{p === "claude" ? "Claude Code" : "Codex"}</div>
                <div className="pc-sub">
                  {!st ? "Checking…" : !ok ? "Not installed" : p === "codex" ? (st.auth ?? "Ready") : "Pro subscription"}
                </div>
              </div>
              <div className="pc-ver">{st?.version?.replace(/[^\d.]/g, "").slice(0, 10)}</div>
            </div>
          );
        })}
      </div>
    </aside>
  );
}
