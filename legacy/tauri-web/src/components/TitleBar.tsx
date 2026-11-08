import { Minus, PanelLeft, PanelRight, Square, X } from "lucide-react";
import { isTauri } from "../lib/agent";
import { useStore } from "../lib/store";

async function win() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

export function TitleBar() {
  const session = useStore((s) => s.active());
  const running = useStore((s) => !!s.runs[s.activeId]);
  const toggleRail = useStore((s) => s.toggleRail);
  const toggleSidebar = useStore((s) => s.toggleSidebar);

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-left">
        <button className="icon-btn" onClick={toggleSidebar} title="Toggle sidebar (Ctrl+B)">
          <PanelLeft size={15} />
        </button>
      </div>
      <div className="tb-center" data-tauri-drag-region>
        <span className={`tb-status ${running ? "live" : ""}`} />
        <span className="tb-title" data-tauri-drag-region>{session.title}</span>
        {session.cwd && (
          <span className="tb-cwd" data-tauri-drag-region>
            {session.cwd.split(/[\\/]/).filter(Boolean).pop()}
          </span>
        )}
      </div>
      <div className="tb-right">
        <button className="icon-btn" onClick={toggleRail} title="Toggle activity (Ctrl+.)">
          <PanelRight size={15} />
        </button>
        {isTauri && (
          <div className="win-controls">
            <button onClick={async () => (await win()).minimize()} title="Minimize"><Minus size={14} /></button>
            <button onClick={async () => (await win()).toggleMaximize()} title="Maximize"><Square size={11} /></button>
            <button className="close" onClick={async () => (await win()).close()} title="Close"><X size={15} /></button>
          </div>
        )}
      </div>
    </header>
  );
}
