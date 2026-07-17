import { useEffect, useRef } from "react";

interface VuMeterProps {
  /** Peak amplitude target 0–1 (real level from the native backend). */
  target: number;
}

/** Meter height (0–1) for a dBFS value, matching the sqrt display curve:
 * height = sqrt(amplitude) = 10^(dB/40). */
const heightForDb = (db: number) => Math.pow(10, db / 40);

/** −6 dBFS reference tick (the red zone above −3 dBFS lives in the CSS
 * gradient stops, which use the same mapping). */
const TICK_6DB = heightForDb(-6) * 100; // ≈ 70.8%
/** Clip latch threshold ≈ −0.2 dBFS (in height space). */
const CLIP_AT = heightForDb(-0.2);

/**
 * Live level meter, calibrated in dBFS. Targets arrive at 10 Hz from the
 * backend's `levels` events; an rAF loop smooths toward them (fast attack,
 * slow release) outside React state. Green below −6 dB, amber to −3 dB,
 * red above - and a clip light that latches for 1.5 s when the signal
 * touches 0 dBFS. The readout shows the held peak in dBFS.
 * Under the pactl fallback no events arrive and the meter rests at zero.
 */
export function VuMeter({ target }: Readonly<VuMeterProps>) {
  const fillRef = useRef<HTMLDivElement>(null);
  const peakRef = useRef<HTMLDivElement>(null);
  const clipRef = useRef<HTMLDivElement>(null);
  const dbRef = useRef<HTMLDivElement>(null);
  const targetRef = useRef(0);
  targetRef.current = target;

  useEffect(() => {
    let raf = 0;
    let smooth = 0;
    let peak = 0;
    let clipUntil = 0;
    let lastDbText = "";
    const tick = () => {
      const t = targetRef.current;
      smooth += (t - smooth) * (t > smooth ? 0.5 : 0.12);
      peak = Math.max(peak * 0.985, smooth);
      if (t >= CLIP_AT) clipUntil = performance.now() + 1500;

      if (fillRef.current) {
        fillRef.current.style.clipPath = `inset(${(100 - smooth * 100).toFixed(1)}% 0 0 0)`;
      }
      if (peakRef.current) peakRef.current.style.bottom = (peak * 100).toFixed(1) + "%";
      if (clipRef.current) {
        clipRef.current.className =
          "vu-clip" + (performance.now() < clipUntil ? " on" : "");
      }
      if (dbRef.current) {
        // Held peak in dBFS (height is sqrt(amplitude), so dB = 40·log10).
        const text = peak < 0.02 ? "−∞" : String(Math.round(40 * Math.log10(peak)));
        if (text !== lastDbText) {
          lastDbText = text;
          dbRef.current.textContent = text;
        }
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="vu-col" title="Peak level in dBFS - tick at −6, red above −3, light latches on clipping">
      <div className="vu-clip" ref={clipRef} />
      <div className="meter">
        <div className="meter-fill" ref={fillRef} />
        <div className="meter-tick" style={{ bottom: `${TICK_6DB}%` }} />
        <div className="meter-peak" ref={peakRef} style={{ bottom: "0%" }} />
      </div>
      <div className="vu-db" ref={dbRef}>
        −∞
      </div>
    </div>
  );
}
