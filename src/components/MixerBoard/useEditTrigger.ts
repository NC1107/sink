import { useRef } from "react";
import type { KeyboardEvent, RefObject } from "react";

interface EditTrigger {
  props: {
    ref: RefObject<HTMLDivElement | null>;
    role: "button";
    tabIndex: 0;
    onDoubleClick: () => void;
    onKeyDown: (e: KeyboardEvent) => void;
  };
  /** Call when the edit closes; hands focus back only if the keyboard opened it. */
  restoreFocus: () => void;
}

/**
 * Makes an inline-editable label reachable without a mouse: Enter or Space
 * opens it, the same as a double-click does.
 */
export function useEditTrigger(start: () => void): EditTrigger {
  const ref = useRef<HTMLDivElement>(null);
  const openedByKey = useRef(false);

  return {
    props: {
      ref,
      role: "button",
      tabIndex: 0,
      onDoubleClick: () => {
        openedByKey.current = false;
        start();
      },
      onKeyDown: (e) => {
        if (e.key !== "Enter" && e.key !== " ") return;
        e.preventDefault();
        openedByKey.current = true;
        start();
      },
    },
    restoreFocus: () => {
      if (!openedByKey.current) return;
      // The trigger is remounted by the same render that closes the input.
      requestAnimationFrame(() => ref.current?.focus());
    },
  };
}
