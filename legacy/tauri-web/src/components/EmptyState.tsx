import { Bug, FlaskConical, Map, Wand2 } from "lucide-react";
import { useStore } from "../lib/store";
import { Orb } from "./Orb";

const IDEAS = [
  { icon: Map, title: "Map this codebase", prompt: "Give me a tour of this codebase: architecture, key modules, and how data flows through it." },
  { icon: Bug, title: "Hunt a bug", prompt: "Find the most likely bugs in this project and explain how you'd fix each one." },
  { icon: FlaskConical, title: "Write tests", prompt: "Find the least-tested critical code here and write focused tests for it." },
  { icon: Wand2, title: "Polish & refactor", prompt: "Suggest the three highest-impact refactors in this project, then do the first one." },
];

export function EmptyState() {
  const session = useStore((s) => s.active());
  const send = useStore((s) => s.send);
  const hour = new Date().getHours();
  const greet = hour < 5 ? "Burning the midnight oil" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";

  return (
    <div className="empty">
      <Orb size={176} />
      <div className="empty-kicker">{greet}</div>
      <h1 className="empty-title">
        What are we <span className="shimmer-text">building</span>?
      </h1>
      <p className="empty-sub">
        {session.provider === "claude" ? "Claude Code" : "Codex"} is standing by
        {session.cwd ? <> in <b>{session.cwd.split(/[\\/]/).filter(Boolean).pop()}</b></> : <> — pick a workspace folder below</>}.
        Switch brains anytime with <kbd>Ctrl J</kbd>.
      </p>
      <div className="ideas">
        {IDEAS.map(({ icon: Icon, title, prompt }, i) => (
          <button key={title} className="idea glass" style={{ animationDelay: `${120 + i * 70}ms` }} onClick={() => send(prompt)}>
            <span className="idea-icon"><Icon size={16} /></span>
            <span className="idea-title">{title}</span>
            <span className="idea-prompt">{prompt}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
