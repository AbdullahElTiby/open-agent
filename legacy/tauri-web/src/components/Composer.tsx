import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowUp, ChevronDown, FolderOpen, Gauge, Square } from "lucide-react";
import { isTauri } from "../lib/agent";
import { MODES, PROVIDERS } from "../lib/models";
import { useStore } from "../lib/store";
import type { Mode, Provider } from "../lib/types";

function Select({
  value, options, onChange, icon, title,
}: { value: string; options: { id: string; label: string }[]; onChange: (v: string) => void; icon?: any; title: string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, []);
  const cur = options.find((o) => o.id === value) ?? { id: value, label: value };
  const Icon = icon;
  return (
    <div className="select" ref={ref}>
      <button className="chip" onClick={() => setOpen(!open)} title={title}>
        {Icon && <Icon size={13} />}
        <span>{cur.label}</span>
        <ChevronDown size={12} />
      </button>
      {open && (
        <div className="menu glass">
          <div className="menu-title">{title}</div>
          {options.map((o) => (
            <button key={o.id} className={o.id === value ? "on" : ""} onClick={() => { onChange(o.id); setOpen(false); }}>
              {o.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Composer() {
  const session = useStore((s) => s.active());
  const running = useStore((s) => !!s.runs[s.activeId]);
  const codexModels = useStore((s) => s.providers.codex?.models);
  const { update, send, stop } = useStore.getState();
  const [text, setText] = useState("");
  const ta = useRef<HTMLTextAreaElement>(null);
  const p = session.provider;

  useLayoutEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = Math.min(el.scrollHeight, 260) + "px";
  }, [text]);

  useEffect(() => ta.current?.focus(), [session.id, p]);

  const submit = () => {
    if (running || !text.trim()) return;
    send(text);
    setText("");
  };

  const pickFolder = async () => {
    if (!isTauri) {
      const v = prompt("Workspace folder path", session.cwd);
      if (v != null) update({ cwd: v });
      return;
    }
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, defaultPath: session.cwd || undefined });
    if (typeof dir === "string") update({ cwd: dir, threads: {} });
  };

  const models =
    p === "codex" && codexModels?.length
      ? [{ id: "default", label: "Default" }, ...codexModels.map((m) => ({ id: m, label: m }))]
      : PROVIDERS[p].models;

  return (
    <div className="composer-wrap">
      <div className={`composer ${running ? "busy" : ""}`}>
        <div className="composer-glow" />
        <textarea
          ref={ta}
          value={text}
          placeholder={running ? `${PROVIDERS[p].label} is working… (Esc to stop)` : `Ask ${PROVIDERS[p].label} to build, fix, or explain anything…`}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
          rows={1}
        />
        <div className="composer-bar">
          <div className="switch" data-p={p} title="Switch agent (Ctrl+J)">
            <div className="switch-thumb" />
            {(["claude", "codex"] as Provider[]).map((id) => (
              <button key={id} className={p === id ? "on" : ""} onClick={() => update({ provider: id })} disabled={running}>
                <span className={`p-dot ${id} on`} />
                {PROVIDERS[id].label}
              </button>
            ))}
          </div>

          <Select
            title={`${PROVIDERS[p].label} model`}
            value={session.models[p]}
            options={models}
            onChange={(v) => update({ models: { ...session.models, [p]: v } })}
          />
          <Select
            title="Reasoning effort"
            icon={Gauge}
            value={session.effort[p]}
            options={PROVIDERS[p].efforts.map((e) => ({ id: e, label: e === "default" ? "Auto" : e[0].toUpperCase() + e.slice(1) }))}
            onChange={(v) => update({ effort: { ...session.effort, [p]: v } })}
          />

          <div className="modes">
            {(Object.keys(MODES) as Mode[]).map((m) => (
              <button
                key={m}
                className={`mode ${m} ${session.mode === m ? "on" : ""}`}
                onClick={() => update({ mode: m })}
                title={MODES[m].hint}
              >
                {MODES[m].label}
              </button>
            ))}
          </div>

          <div className="spacer" />

          <button className="chip folder" onClick={pickFolder} title={session.cwd || "Choose workspace folder"}>
            <FolderOpen size={13} />
            <span>{session.cwd ? session.cwd.split(/[\\/]/).filter(Boolean).pop() : "Workspace"}</span>
          </button>

          {running ? (
            <button className="send stop" onClick={stop} title="Stop (Esc)">
              <Square size={14} fill="currentColor" />
            </button>
          ) : (
            <button className="send" onClick={submit} disabled={!text.trim()} title="Send (Enter)">
              <ArrowUp size={18} strokeWidth={2.4} />
            </button>
          )}
        </div>
      </div>
      <div className="composer-hint">
        <span className={`mode-hint ${session.mode}`}>{MODES[session.mode].hint}</span>
      </div>
    </div>
  );
}
