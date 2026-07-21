import { useEffect, useRef, useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";

/**
 * ChatMix-style balance between two user-picked channels. Stateless: the
 * slider is a macro over the two faders - center = both at 100%, sliding
 * toward a side ducks the OTHER one (silent at the extreme). Position is
 * always derived from the two volumes, so hand-moving a fader moves the
 * balance too, and profiles capture it for free.
 */
export function BalanceBar() {
  const channels = useMixerStore((s) => s.channels);
  const balanceA = useMixerStore((s) => s.balanceA);
  const balanceB = useMixerStore((s) => s.balanceB);
  const showBalance = useMixerStore((s) => s.showBalance);
  const setBalanceChannels = useMixerStore((s) => s.setBalanceChannels);
  const setChannelVolume = useMixerStore((s) => s.setChannelVolume);

  // Resolve picks: saved choices when they still exist, else Game/Chat,
  // else the first two channels.
  const find = (name: string | null) => channels.find((c) => c.name === name) ?? null;
  let a = find(balanceA);
  let b = find(balanceB);
  if (!a || !b || a.name === b.name) {
    const game = find("sink_game");
    const chat = find("sink_chat");
    a = a ?? game ?? channels[0] ?? null;
    b = b ?? chat ?? channels.find((c) => c.name !== a?.name) ?? null;
  }

  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const [pickingA, setPickingA] = useState(false);
  const [pickingB, setPickingB] = useState(false);

  // pos ∈ [−1, +1]: + favors B (A ducked), − favors A (B ducked).
  const pos = a && b ? (b.volume_percent - a.volume_percent) / 100 : 0;

  const apply = (p: number) => {
    if (!a || !b) return;
    const clamped = Math.max(-1, Math.min(1, p));
    // Snap to true center near the middle.
    const snapped = Math.abs(clamped) < 0.04 ? 0 : clamped;
    void setChannelVolume(a.name, Math.round(100 * Math.min(1, 1 - snapped)));
    void setChannelVolume(b.name, Math.round(100 * Math.min(1, 1 + snapped)));
  };

  const fromEvent = (clientX: number) => {
    const el = trackRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    apply(((clientX - r.left) / r.width) * 2 - 1);
  };
  const fromEventRef = useRef(fromEvent);
  fromEventRef.current = fromEvent;

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragging.current) fromEventRef.current(e.clientX);
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

  if (!showBalance || !a || !b || channels.length < 2) return null;

  const side = (
    channel: VirtualSink,
    open: boolean,
    setOpen: (v: boolean) => void,
    other: VirtualSink,
    pick: (name: string) => void,
  ) => (
    <div style={{ position: "relative", display: "flex" }}>
      <button
        className="bal-side"
        onClick={() => setOpen(!open)}
        title={`${channel.label} - click to pick the channel on this side`}
      >
        <Ms name={channel.icon ?? "graphic_eq"} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="center">
        {channels
          .filter((c) => c.name !== other.name)
          .map((c) => (
            <MenuItem
              key={c.name}
              icon={c.icon ?? "graphic_eq"}
              selected={c.name === channel.name}
              onClick={() => {
                pick(c.name);
                setOpen(false);
              }}
            >
              {c.label}
            </MenuItem>
          ))}
      </Popover>
    </div>
  );

  return (
    <div className="balance-bar" title="Balance - center is both at 100%; double-click to recenter">
      {side(a, pickingA, setPickingA, b, (name) => void setBalanceChannels(name, b!.name))}
      <div
        className="bal-track"
        ref={trackRef}
        title={`${a.label} ${a.volume_percent}% / ${b.label} ${b.volume_percent}% - slide toward a side to duck the other`}
        onPointerDown={(e) => {
          dragging.current = true;
          fromEvent(e.clientX);
        }}
        onDoubleClick={() => apply(0)}
      >
        <div className="bal-center" />
        <div className="bal-cap" style={{ left: `${((pos + 1) / 2) * 100}%` }} />
      </div>
      {side(b, pickingB, setPickingB, a, (name) => void setBalanceChannels(a!.name, name))}
    </div>
  );
}
