import { useEffect } from "react";
import type { RefObject } from "react";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { nextWidth, windowSize } from "../lib/layout";

const LAST_NATURAL_KEY = "sink.window.naturalWidth";

function readLastNatural(): number | null {
  try {
    const v = Number(localStorage.getItem(LAST_NATURAL_KEY));
    return Number.isFinite(v) && v > 0 ? v : null;
  } catch {
    return null;
  }
}

/** Size the window to the board on first launch, and keep its width
 * following the board afterwards unless the user narrowed it. The chrome
 * around the board is measured rather than assumed. */
export function useWindowToBoard(viewport: RefObject<HTMLElement | null>, boardW: number) {
  useEffect(() => {
    const view = viewport.current;
    if (!view || boardW <= 0) return;
    let cancelled = false;
    const cs = getComputedStyle(view);
    const availW = view.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const chromeW = window.innerWidth - availW;
    void currentMonitor().then(async (monitor) => {
      if (cancelled) return;
      const scale = monitor?.scaleFactor ?? 1;
      const work = monitor
        ? { width: monitor.workArea.size.width / scale, height: monitor.workArea.size.height / scale }
        : { width: Number.MAX_SAFE_INTEGER, height: Number.MAX_SAFE_INTEGER };
      const natural = windowSize(boardW, chromeW, work);
      const last = readLastNatural();
      const width = nextWidth(window.innerWidth, natural.width, last);
      if (width !== null) {
        // Only a first launch takes the designed height; after that it's the user's.
        const height = last === null ? natural.height : window.innerHeight;
        await getCurrentWindow().setSize(new LogicalSize(width, height));
      }
      try {
        localStorage.setItem(LAST_NATURAL_KEY, String(natural.width));
      } catch {
        // Per-viewer convenience only; nothing to do without storage.
      }
    });
    return () => {
      cancelled = true;
    };
  }, [viewport, boardW]);
}
