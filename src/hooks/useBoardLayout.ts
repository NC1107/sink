import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";
import { stripHeight } from "../lib/layout";

export interface BoardLayout {
  /** Fader length that fills the viewport. */
  stripHeight: number;
  /** The board's natural width, what the window is sized around. */
  width: number;
}

/** Measures the board against its viewport; re-measures when the viewport
 * resizes or `deps` change the board's content. */
export function useBoardLayout(
  viewport: RefObject<HTMLElement | null>,
  board: RefObject<HTMLElement | null>,
  deps: readonly unknown[],
): BoardLayout {
  const [layout, setLayout] = useState<BoardLayout>({ stripHeight: 0, width: 0 });
  useLayoutEffect(() => {
    const view = viewport.current;
    const el = board.current;
    if (!view || !el) return;
    const measure = () => {
      const cs = getComputedStyle(view);
      const availH = view.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
      const strip = el.querySelector<HTMLElement>(".strip");
      const chromeH = el.offsetHeight - (strip?.offsetHeight ?? 0);
      const next = { stripHeight: stripHeight(availH, chromeH), width: el.offsetWidth };
      setLayout((prev) =>
        prev.stripHeight === next.stripHeight && prev.width === next.width ? prev : next,
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(view);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return layout;
}
