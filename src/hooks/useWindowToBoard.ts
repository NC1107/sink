import { useEffect } from "react";
import type { RefObject } from "react";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { MIN_WINDOW_WIDTH, nextWidth, windowSize } from "../lib/layout";

const LAST_NATURAL_KEY = "sink.window.naturalWidth";

function readLastNatural(): number | null {
  try {
    const v = Number(localStorage.getItem(LAST_NATURAL_KEY));
    return Number.isFinite(v) && v > 0 ? v : null;
  } catch {
    return null;
  }
}

/** Keep the window's height fixed and its width following the board,
 * unless the user narrowed it, in which case the board scrolls. The chrome
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
      const win = getCurrentWindow();
      // Width is the user's between the minimum and the board; height is not.
      await win.setMinSize(new LogicalSize(MIN_WINDOW_WIDTH, natural.height));
      await win.setMaxSize(new LogicalSize(Math.max(natural.width, MIN_WINDOW_WIDTH), natural.height));
      const width = nextWidth(window.innerWidth, natural.width, readLastNatural());
      if (width !== null) await win.setSize(new LogicalSize(width, natural.height));
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
