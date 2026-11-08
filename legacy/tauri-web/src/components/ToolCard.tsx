import { useState } from "react";
import {
  BookOpen, ChevronRight, FilePen, FilePlus2, FileText, FolderSearch, Globe, ListChecks,
  Plug, Search, SquareTerminal, Wrench, Bot, Sparkles,
} from "lucide-react";
import type { Part } from "../lib/types";

type Tool = Extract<Part, { type: "tool" }>;

const short = (p: string) => {
  const parts = String(p).split(/[\\/]/).filter(Boolean);
  return parts.length > 3 ? "…/" + parts.slice(-3).join("/") : String(p);
};

function describe(t: Tool): { icon: any; verb: string; target: string; kind: string } {
  const i = t.input ?? {};
  switch (t.name) {
    case "Read": return { icon: FileText, verb: "Read", target: short(i.file_path), kind: "read" };
    case "Write": return { icon: FilePlus2, verb: "Write", target: short(i.file_path), kind: "edit" };
    case "Edit": case "MultiEdit": return { icon: FilePen, verb: "Edit", target: short(i.file_path), kind: "edit" };
    case "NotebookEdit": return { icon: FilePen, verb: "Edit notebook", target: short(i.notebook_path), kind: "edit" };
    case "Bash": case "PowerShell": case "shell":
      return { icon: SquareTerminal, verb: "Run", target: i.command ?? "", kind: "shell" };
    case "Glob": return { icon: FolderSearch, verb: "Find files", target: i.pattern, kind: "search" };
    case "Grep": return { icon: Search, verb: "Search", target: i.pattern, kind: "search" };
    case "WebFetch": return { icon: Globe, verb: "Fetch", target: i.url, kind: "web" };
    case "WebSearch": case "web_search": return { icon: Globe, verb: "Web search", target: i.query, kind: "web" };
    case "Task": case "Agent": return { icon: Bot, verb: "Subagent", target: i.description ?? "", kind: "agent" };
    case "Skill": return { icon: Sparkles, verb: "Skill", target: i.skill ?? "", kind: "agent" };
    case "file_change": {
      const ch = (i.changes ?? []) as { path: string; kind: string }[];
      return { icon: FilePen, verb: ch.length > 1 ? `Change ${ch.length} files` : `${cap(ch[0]?.kind ?? "edit")}`, target: ch.length === 1 ? short(ch[0].path) : ch.map((c) => short(c.path)).join(", "), kind: "edit" };
    }
    case "ExitPlanMode": return { icon: ListChecks, verb: "Plan ready", target: "", kind: "agent" };
    default:
      if (t.name.startsWith("mcp")) return { icon: Plug, verb: t.name.replace(/^mcp[:_]+/, "").replace(/__/g, " / "), target: "", kind: "web" };
      if (t.name === "LS") return { icon: BookOpen, verb: "List", target: short(i.path), kind: "read" };
      return { icon: Wrench, verb: t.name, target: "", kind: "other" };
  }
}

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

function Diff({ oldStr, newStr }: { oldStr: string; newStr: string }) {
  const o = (oldStr ?? "").split("\n");
  const n = (newStr ?? "").split("\n");
  return (
    <pre className="diff">
      {o.map((l, k) => <div key={"o" + k} className="del">- {l}</div>)}
      {n.map((l, k) => <div key={"n" + k} className="add">+ {l}</div>)}
    </pre>
  );
}

function Body({ t }: { t: Tool }) {
  const i = t.input ?? {};
  if ((t.name === "Edit") && i.old_string !== undefined)
    return <Diff oldStr={i.old_string} newStr={i.new_string} />;
  if (t.name === "MultiEdit" && Array.isArray(i.edits))
    return <>{i.edits.map((e: any, k: number) => <Diff key={k} oldStr={e.old_string} newStr={e.new_string} />)}</>;
  if (t.name === "Write" && i.content)
    return <pre className="tool-out">{String(i.content).slice(0, 6000)}</pre>;
  if (t.name === "file_change")
    return (
      <div className="changes">
        {(i.changes ?? []).map((c: any, k: number) => (
          <div key={k}><span className={`chg ${c.kind}`}>{c.kind}</span> {c.path}</div>
        ))}
      </div>
    );
  const isShell = ["Bash", "PowerShell", "shell"].includes(t.name);
  return (
    <>
      {isShell ? (
        <pre className="tool-cmd"><span className="prompt">❯</span> {i.command}</pre>
      ) : t.input && Object.keys(i).length > 0 && t.name !== "Read" ? (
        <pre className="tool-in">{JSON.stringify(t.input, null, 2)}</pre>
      ) : null}
      {t.output ? <pre className={`tool-out ${t.status === "error" ? "err" : ""}`}>{t.output.slice(0, 12000)}</pre> : null}
    </>
  );
}

export function ToolCard({ t }: { t: Tool }) {
  const [open, setOpen] = useState(false);
  const d = describe(t);
  const Icon = d.icon;
  return (
    <div className={`tool ${t.status} k-${d.kind} ${open ? "open" : ""}`}>
      <button className="tool-head" onClick={() => setOpen(!open)}>
        <span className="tool-icon"><Icon size={14} /></span>
        <span className="tool-verb">{d.verb}</span>
        {d.target && <span className="tool-target">{d.target}</span>}
        <span className="tool-state" />
        <ChevronRight size={14} className="tool-chev" />
      </button>
      {open && <div className="tool-body"><Body t={t} /></div>}
      {t.status === "running" && <div className="tool-scan" />}
    </div>
  );
}
