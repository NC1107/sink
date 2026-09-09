import { useEffect } from "react";
import type { RefObject } from "react";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { windowSize } from "../lib/layout";

/** Size the window so a `boardW`-wide board shows at 1x, within the
 * monitor's work area. The chrome around the board is measured from the
 * viewport rather than assumed, so rail and padding changes follow. */
export function useWindowToBoard(viewport: RefObject<HTMLElement | null>, boardW: number) {
  useEffect(() => {
    const view = viewport.current;
    if (!view || boardW <= 0) return;
    let cancelled = false;
    const cs = getComputedStyle(view);
    const availW = view.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const chromeW = window.innerWidth - availW;
    void currentMonitor().then((monitor) => {
      if (cancelled) return;
      const scale = monitor?.scaleFactor ?? 1;
      const work = monitor
        ? { width: monitor.workArea.size.width / scale, height: monitor.workArea.size.height / scale }
        : { width: Number.MAX_SAFE_INTEGER, height: Number.MAX_SAFE_INTEGER };
      const size = windowSize(boardW, chromeW, work);
      void getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
    });
    return () => {
      cancelled = true;
    };
  }, [viewport, boardW]);
}
