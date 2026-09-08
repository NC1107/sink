import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";
import { fitScale } from "../lib/layout";

export interface Fit {
  scale: number;
  /** The board's unscaled size, so its box can be sized to the scaled one. */
  width: number;
  height: number;
}

/** Scale `board` up to fill `viewport` (minus padding); re-measures when
 * the viewport resizes or `deps` change the board's content. */
export function useFitScale(
  viewport: RefObject<HTMLElement>,
  board: RefObject<HTMLElement>,
  deps: readonly unknown[],
): Fit {
  const [fit, setFit] = useState<Fit>({ scale: 1, width: 0, height: 0 });
  useLayoutEffect(() => {
    const view = viewport.current;
    const el = board.current;
    if (!view || !el) return;
    const measure = () => {
      const cs = getComputedStyle(view);
      const availW = view.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
      const availH = view.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
      // offsetWidth/Height ignore the transform, so this is the natural size.
      const { offsetWidth: width, offsetHeight: height } = el;
      const scale = fitScale(availW, availH, width, height);
      setFit((prev) =>
        prev.scale === scale && prev.width === width && prev.height === height
          ? prev
          : { scale, width, height },
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
