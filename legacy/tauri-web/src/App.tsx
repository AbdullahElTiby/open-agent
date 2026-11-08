import { useEffect } from "react";
import { Aurora } from "./components/Aurora";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { Conversation } from "./components/Conversation";
import { Composer } from "./components/Composer";
import { ActivityRail } from "./components/ActivityRail";
import { checkProviders } from "./lib/agent";
import { useStore } from "./lib/store";

export default function App() {
  const session = useStore((s) => s.active());
  const running = useStore((s) => !!s.runs[s.activeId]);
  const railOpen = useStore((s) => s.railOpen);
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const setProviders = useStore((s) => s.setProviders);

  useEffect(() => {
    checkProviders().then(setProviders).catch(() => {});
  }, [setProviders]);

  // Global shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") { e.preventDefault(); st.newSession(); }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b") { e.preventDefault(); st.toggleSidebar(); }
      if ((e.ctrlKey || e.metaKey) && e.key === ".") { e.preventDefault(); st.toggleRail(); }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "j") {
        e.preventDefault();
        st.update({ provider: st.active().provider === "claude" ? "codex" : "claude" });
      }
      if (e.key === "Escape" && st.runs[st.activeId]) st.stop();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Glass panels catch a soft spotlight that follows the cursor.
  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      document.documentElement.style.setProperty("--mx", `${e.clientX}px`);
      document.documentElement.style.setProperty("--my", `${e.clientY}px`);
    };
    window.addEventListener("pointermove", onMove);
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  return (
    <div
      className="app"
      data-provider={session.provider}
      data-running={running || undefined}
      data-mode={session.mode}
    >
      <Aurora />
      <TitleBar />
      <div className={`shell ${sidebarOpen ? "" : "no-sidebar"} ${railOpen ? "" : "no-rail"}`}>
        <Sidebar />
        <main className="stage">
          <Conversation />
          <Composer />
        </main>
        <ActivityRail />
      </div>
    </div>
  );
}
