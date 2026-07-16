import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { EqBand, EqConfig, VirtualSink } from "../../types";
import { defaultEqConfig, MAX_EQ_BANDS } from "../../types";
import { Modal } from "../Modal";
import { Ms } from "../Icons";
import { Toggle } from "../Toggle";
import { DspSlider } from "../Mic/DspSlider";
import { EqBandRow } from "./EqBandRow";
import { EqCurve } from "./EqCurve";

interface EqModalProps {
  channel: VirtualSink;
  open: boolean;
  onClose: () => void;
}

/** Per-channel parametric EQ editor: response curve, band list, preamp. */
export function EqModal({ channel, open, onClose }: EqModalProps) {
  const config = useMixerStore(
    (s) => s.eqConfigs[channel.name] ?? null,
  ) ?? defaultEqConfig();
  const setChannelEq = useMixerStore((s) => s.setChannelEq);
  const backendNative = useMixerStore((s) => s.backendNative);
  const [selected, setSelected] = useState(0);

  const apply = (next: EqConfig) => void setChannelEq(channel.name, next);

  const patchBand = (index: number, patch: Partial<EqBand>) => {
    const bands = config.bands.map((b, i) => (i === index ? { ...b, ...patch } : b));
    apply({ ...config, bands });
  };

  const addBand = () => {
    if (config.bands.length >= MAX_EQ_BANDS) return;
    const bands = [...config.bands, { kind: "peaking" as const, freq_hz: 1000, gain_db: 0, q: 1 }];
    apply({ ...config, bands });
    setSelected(bands.length - 1);
  };

  const removeBand = (index: number) => {
    if (config.bands.length <= 1) return;
    const bands = config.bands.filter((_, i) => i !== index);
    apply({ ...config, bands });
    setSelected(Math.min(selected, bands.length - 1));
  };

  const reset = () => {
    // Back to the flat starting layout; the enable switch is left alone.
    apply({ ...defaultEqConfig(), enabled: config.enabled });
    setSelected(0);
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`${channel.label} — Equalizer`}
      className="eqm-modal"
    >
      {backendNative === false && (
        <p className="modal-text">
          Parametric EQ requires the native PipeWire engine, which isn't
          running on this system.
        </p>
      )}
      <div className="eqm-head">
        <div className="eqm-enable">
          <Toggle
            on={config.enabled}
            onClick={() => apply({ ...config, enabled: !config.enabled })}
          />
          <span>{config.enabled ? "On" : "Off"}</span>
        </div>
        <button className="select" onClick={reset} title="Back to the flat 5-band layout">
          <Ms name="restart_alt" style={{ fontSize: 15 }} />
          <span>Reset</span>
        </button>
      </div>

      <EqCurve
        config={config}
        selected={selected}
        onSelect={setSelected}
        onBandChange={patchBand}
      />

      <DspSlider
        label="Preamp"
        min={-24}
        max={24}
        step={0.5}
        unit=" dB"
        value={config.preamp_db}
        defaultValue={0}
        onChange={(v) => apply({ ...config, preamp_db: v })}
      />

      <div className="eqm-bands">
        {config.bands.map((band, i) => (
          <EqBandRow
            key={i}
            band={band}
            selected={i === selected}
            onSelect={() => setSelected(i)}
            onChange={(patch) => patchBand(i, patch)}
            onRemove={() => removeBand(i)}
          />
        ))}
        {config.bands.length < MAX_EQ_BANDS && (
          <button className="eqm-add" onClick={addBand}>
            <Ms name="add" style={{ fontSize: 15 }} />
            Add band
          </button>
        )}
      </div>
    </Modal>
  );
}
