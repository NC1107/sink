import { useCallback, useEffect, useRef } from "react";

interface HSliderProps {
  value: number;
  max: number;
  onChange: (value: number) => void;
}

/** Horizontal per-app volume slider. */
export function HSlider({ value, max, onChange }: Readonly<HSliderProps>) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const setFromEvent = useCallback(
    (clientX: number) => {
      const el = trackRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const pct = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
      onChange(Math.round(pct * max));
    },
    [onChange, max],
  );

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragging.current) setFromEvent(e.clientX);
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
  }, [setFromEvent]);

  const pct = (Math.max(0, Math.min(max, value)) / max) * 100;

  return (
    <div className="hslider">
      <div
        className="hs-track"
        ref={trackRef}
        onPointerDown={(e) => {
          dragging.current = true;
          setFromEvent(e.clientX);
        }}
      >
        <div className="hs-fill" style={{ width: pct + "%" }} />
        <div
          className="hs-cap"
          style={{ left: pct + "%" }}
          onPointerDown={(e) => {
            e.stopPropagation();
            dragging.current = true;
          }}
        />
      </div>
      <div className="hs-val">{value}%</div>
    </div>
  );
}
