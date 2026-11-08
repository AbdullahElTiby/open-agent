import type { Mode, Provider } from "./types";

export const PROVIDERS: Record<
  Provider,
  { label: string; vendor: string; models: { id: string; label: string }[]; efforts: string[] }
> = {
  claude: {
    label: "Claude",
    vendor: "Claude Code subscription",
    models: [
      { id: "default", label: "Default" },
      { id: "opus", label: "Opus" },
      { id: "sonnet", label: "Sonnet" },
      { id: "haiku", label: "Haiku" },
    ],
    efforts: ["default", "low", "medium", "high", "xhigh", "max"],
  },
  codex: {
    label: "Codex",
    vendor: "ChatGPT subscription",
    models: [
      // Replaced at runtime by the models in ~/.codex/models_cache.json.
      { id: "default", label: "Default" },
    ],
    efforts: ["default", "low", "medium", "high", "xhigh", "max"],
  },
};

export const MODES: Record<Mode, { label: string; hint: string }> = {
  plan: { label: "Plan", hint: "Read-only. Explores and proposes, never edits." },
  build: { label: "Build", hint: "Edits files in the workspace. Claude asks before shell; Codex runs shell in a sandbox." },
  auto: { label: "Autopilot", hint: "Full autonomy: no prompts, no sandbox. Use on code you can roll back." },
};
