import { create } from "zustand";

export type ThemeId = "original" | "tokyo-night";

export const THEMES: { id: ThemeId; label: string; swatch: string[] }[] = [
  { id: "original", label: "Original", swatch: ["#0a0a0b", "#5557e0", "#ededef"] },
  { id: "tokyo-night", label: "Tokyo Night", swatch: ["#1a1b26", "#7aa2f7", "#bb9af7"] },
];

const STORAGE_KEY = "sink-theme";

function apply(theme: ThemeId) {
  const root = document.documentElement;
  if (theme === "original") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

function initial(): ThemeId {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "tokyo-night" || saved === "original" ? saved : "original";
}

interface ThemeState {
  theme: ThemeId;
  setTheme: (t: ThemeId) => void;
}

export const useTheme = create<ThemeState>((set) => ({
  theme: initial(),
  setTheme: (t) => {
    localStorage.setItem(STORAGE_KEY, t);
    apply(t);
    set({ theme: t });
  },
}));

/** Apply the persisted theme as early as possible (called from main). */
export function bootTheme() {
  apply(initial());
}
