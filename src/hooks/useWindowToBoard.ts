import { useEffect } from "react";
import type { RefObject } from "react";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { snappedWidth, windowSize } from "../lib/layout";

const SIZED_KEY = "sink.window.sized";

/** Open the window at the board's 1x size on first launch; later launches
 * keep whatever size the user chose. The chrome around the board is
 * measured rather than assumed. */
export function useWindowToBoard(
  viewport: RefObject<HTMLElement | null>,
  board: RefObject<HTMLElement | null>,
  boardW: number,
) {
  useEffect(() => {
    const view = viewport.current;
    const el = board.current;
    if (!view || !el || boardW <= 0) return;
    let cancelled = false;
    const cs = getComputedStyle(view);
    const availW = view.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const availH = view.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    const chrome = { width: window.innerWidth - availW, height: window.innerHeight - availH };
    const strip = el.querySelector<HTMLElement>(".strip");
    const chromeH = el.offsetHeight - (strip?.offsetHeight ?? 0);
    void currentMonitor().then(async (monitor) => {
      if (cancelled) return;
      const dpi = monitor?.scaleFactor ?? 1;
      const work = monitor
        ? { width: monitor.workArea.size.width / dpi, height: monitor.workArea.size.height / dpi }
        : { width: Number.MAX_SAFE_INTEGER, height: Number.MAX_SAFE_INTEGER };
      let sized = false;
      try {
        sized = localStorage.getItem(SIZED_KEY) === "1";
      } catch {
        // No storage: size every launch, which is still correct.
      }
      if (!sized) {
        const size = windowSize(boardW, chromeH, chrome, work);
        await getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
        try {
          localStorage.setItem(SIZED_KEY, "1");
        } catch {
          // Per-viewer convenience only.
        }
      }
      // The backend keeps the window hidden until the board is sized.
      await emit("sink-ready");
    });
    // A drag can stop anywhere; once it does, land on the nearest step so
    // the board never renders between them (a maximised window stays put).
    const win = getCurrentWindow();
    let settle: number | undefined;
    const unlisten = win.onResized(() => {
      window.clearTimeout(settle);
      settle = window.setTimeout(() => {
        void win.isMaximized().then(async (maximized) => {
          if (cancelled || maximized) return;
          const target = snappedWidth(window.innerWidth, boardW, chrome.width);
          if (Math.abs(target - window.innerWidth) > 1) {
            await win.setSize(new LogicalSize(target, window.innerHeight));
          }
        });
      }, 200);
    });
    return () => {
      cancelled = true;
      window.clearTimeout(settle);
      void unlisten.then((stop) => stop());
    };
  }, [viewport, board, boardW]);
}
