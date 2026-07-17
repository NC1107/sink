import { useCallback, useEffect, useRef } from "react";

interface FaderProps {
  value: number;
  max: number;
  onChange: (value: number) => void;
}

/** Vertical channel fader (pointer-driven, design-system styling). */
export function Fader({ value, max, onChange }: Readonly<FaderProps>) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const setFromEvent = useCallback(
    (clientY: number) => {
      const el = trackRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const pct = Math.max(0, Math.min(1, 1 - (clientY - r.top) / r.height));
      onChange(Math.round(pct * max));
    },
    [onChange, max],
  );

  // Listeners read the latest handler through a ref so they're attached
  // once, not re-registered every parent re-render (faders re-render on
  // each volume tick mid-drag).
  const setFromEventRef = useRef(setFromEvent);
  setFromEventRef.current = setFromEvent;

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragging.current) setFromEventRef.current(e.clientY);
    };
    const up = () => {
      dragging.current = false;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, []);

  const pct = (Math.max(0, Math.min(max, value)) / max) * 100;

  return (
    <div className="fader">
      <div
        className="fader-track"
        ref={trackRef}
        onPointerDown={(e) => {
          dragging.current = true;
          setFromEvent(e.clientY);
        }}
      >
        <div className="fader-fill" style={{ height: pct + "%" }} />
        <div
          className="fader-cap"
          style={{ bottom: pct + "%" }}
          onPointerDown={(e) => {
            e.stopPropagation();
            dragging.current = true;
          }}
        />
      </div>
    </div>
  );
}
