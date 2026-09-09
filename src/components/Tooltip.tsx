import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** How long the pointer must rest on an element before its tooltip appears.
 *  Single source of truth for tooltip timing across the whole app. */
export const TOOLTIP_DELAY_MS = 1200;

/** Gap from the anchor, and minimum inset from the viewport edges (px). */
const GAP = 6;
const MARGIN = 8;

/**
 * One global tooltip that stands in for WebKitGTK's native `title` popups,
 * which appear instantly and can spill off-screen. It reads the same `title`
 * attributes already on elements, waits {@link TOOLTIP_DELAY_MS}, and clamps
 * itself inside the window. The title is stashed while hovered (and restored
 * on leave) so the native tooltip stays suppressed without losing the
 * accessible name assistive tech reads from it.
 */
export function Tooltip() {
  const [text, setText] = useState<string | null>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ el: Element; title: string } | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const restore = () => {
      if (anchor.current) {
        anchor.current.el.setAttribute("title", anchor.current.title);
        anchor.current = null;
      }
    };
    const hide = () => {
      window.clearTimeout(timer.current);
      restore();
      setText(null);
    };

    const onOver = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (!target?.closest) return;
      // Still inside the element we're already tracking: keep it up.
      if (anchor.current?.el.contains(target)) return;
      const el = target.closest("[title]");
      if (!el) return;
      const title = el.getAttribute("title");
      if (!title || !title.trim()) return;
      hide();
      // Stash the title and drop it so the native tooltip can't fire; the
      // layout effect positions ours once the delay elapses.
      anchor.current = { el, title };
      el.removeAttribute("title");
      timer.current = window.setTimeout(() => setText(title), TOOLTIP_DELAY_MS);
    };

    const onOut = (e: MouseEvent) => {
      if (!anchor.current) return;
      const to = e.relatedTarget as Node | null;
      if (e.target === anchor.current.el && !anchor.current.el.contains(to)) hide();
    };

    document.addEventListener("mouseover", onOver, true);
    document.addEventListener("mouseout", onOut, true);
    window.addEventListener("scroll", hide, true);
    document.addEventListener("pointerdown", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      document.removeEventListener("mouseover", onOver, true);
      document.removeEventListener("mouseout", onOut, true);
      window.removeEventListener("scroll", hide, true);
      document.removeEventListener("pointerdown", hide, true);
      window.removeEventListener("blur", hide);
      window.clearTimeout(timer.current);
      restore();
    };
  }, []);

  // Position against the anchor and clamp inside the viewport once sized.
  useLayoutEffect(() => {
    const tip = tipRef.current;
    if (text === null || !tip || !anchor.current) return;
    const a = anchor.current.el.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    let left = a.left + a.width / 2 - t.width / 2;
    left = Math.max(MARGIN, Math.min(left, window.innerWidth - t.width - MARGIN));
    let top = a.bottom + GAP;
    if (top + t.height > window.innerHeight - MARGIN) top = a.top - t.height - GAP;
    top = Math.max(MARGIN, top);
    tip.style.left = `${Math.round(left)}px`;
    tip.style.top = `${Math.round(top)}px`;
    tip.style.visibility = "visible";
  }, [text]);

  if (text === null) return null;
  return createPortal(
    <div ref={tipRef} className="app-tooltip" aria-hidden="true" style={{ visibility: "hidden" }}>
      {text}
    </div>,
    document.body,
  );
}
