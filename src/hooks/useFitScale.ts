import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";
import { fitBoard } from "../lib/layout";
import type { BoardFit } from "../lib/layout";

export interface Fit extends BoardFit {
  /** The board's unscaled size, so its box can be sized to the scaled one. */
  width: number;
  height: number;
}

/** Fit `board` to `viewport` (minus padding); re-measures when the viewport
 * resizes or `deps` change the board's content. */
export function useFitScale(
  viewport: RefObject<HTMLElement | null>,
  board: RefObject<HTMLElement | null>,
  deps: readonly unknown[],
): Fit {
  const [fit, setFit] = useState<Fit>({ scale: 1, stripHeight: 0, width: 0, height: 0 });
  useLayoutEffect(() => {
    const view = viewport.current;
    const el = board.current;
    if (!view || !el) return;
    const measure = () => {
      // Natural sizes: the board is rendered with CSS zoom, which changes
      // its layout box, so measure it at 1x. React won't rewrite an
      // unchanged style prop, so the zoom has to be put back by hand.
      const zoom = el.style.zoom;
      el.style.zoom = "1";
      const cs = getComputedStyle(view);
      const availW = view.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
      const availH = view.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
      // offsetWidth/Height ignore the transform, so these are natural sizes.
      const strip = el.querySelector<HTMLElement>(".strip");
      const chromeH = el.offsetHeight - (strip?.offsetHeight ?? 0);
      const { scale, stripHeight } = fitBoard(availW, availH, el.offsetWidth, chromeH);
      const width = el.offsetWidth;
      const height = chromeH + stripHeight;
      el.style.zoom = zoom;
      setFit((prev) =>
        prev.scale === scale && prev.stripHeight === stripHeight && prev.width === width && prev.height === height
          ? prev
          : { scale, stripHeight, width, height },
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(view);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return fit;
}
