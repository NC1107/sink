import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

/** The board's natural width, re-measured when `deps` change its content. */
export function useBoardWidth(board: RefObject<HTMLElement | null>, deps: readonly unknown[]): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = board.current;
    if (el) setWidth(el.offsetWidth);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return width;
}
