import { useEffect, useRef, useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { MAX_MIC_GAIN, MIC_LEVEL_KEY, MIC_DSP_DEFAULTS } from "../../types";
import { DspSlider } from "./DspSlider";
import { perceptual } from "../../lib/audio";
import { HSlider } from "../AppList/HSlider";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle, ToggleRow } from "../Toggle";

/** Live mono input level driven by the `levels` event stream. */
function MicLevel() {
  const fillRef = useRef<HTMLDivElement>(null);
  const level = useMixerStore((s) => s.levels[MIC_LEVEL_KEY]);
  const target = perceptual(level?.[0] ?? 0);
  const targetRef = useRef(0);
  targetRef.current = target;

  useEffect(() => {
    let raf = 0;
    let smooth = 0;
    const tick = () => {
      const t = targetRef.current;
      smooth += (t - smooth) * (t > smooth ? 0.5 : 0.12);
      if (fillRef.current) fillRef.current.style.width = (smooth * 100).toFixed(1) + "%";
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="mini-meter">
      <div className="mf" ref={fillRef} style={{ width: "0%" }} />
    </div>
  );
}

function micStatusLabel(enabled: boolean, muted: boolean): string {
  if (!enabled) return "Off";
  if (muted) return "Muted";
  return "Live";
}

export function MicScreen() {
  const micConfig = useMixerStore((s) => s.micConfig);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const setMicConfig = useMixerStore((s) => s.setMicConfig);
  const listening = useMixerStore((s) => s.monitors[MIC_LEVEL_KEY] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const [deviceOpen, setDeviceOpen] = useState(false);

  if (!micConfig) {
    return (
      <div className="content">
        <div className="empty-hint" style={{ margin: "auto" }}>
          Loading mic configuration…
        </div>
      </div>
    );
  }

  const currentDevice = inputDevices.find((d) => d.name === micConfig.input_device);
  const deviceLabel =
    micConfig.input_device === null
      ? "System default"
      : (currentDevice?.description ?? micConfig.input_device);

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>Microphone</h1>
        <div className="screen-head-actions">
          <span
            className={
              "tag" + (!micConfig.enabled || micConfig.muted ? " tag-off" : " live")
            }
          >
            <Ms name="fiber_manual_record" style={{ fontSize: 11 }} />
            {micStatusLabel(micConfig.enabled, micConfig.muted)}
          </span>
          <Toggle
            on={micConfig.enabled}
            onClick={() => void setMicConfig({ enabled: !micConfig.enabled })}
          />
        </div>
      </div>
      <div className="screen-scroll">
        <div className={micConfig.enabled ? undefined : "mic-disabled"}>
            <div className="section-label">Input</div>
            <div className="card mic-card">
              <div className="mic-gain-row">
                <span className="mic-gain-label">Name</span>
                <input
                  className="menu-input"
                  style={{ flex: 1 }}
                  value={micConfig.output_label}
                  maxLength={32}
                  title="How other apps list your processed mic"
                  onChange={(e) => void setMicConfig({ output_label: e.target.value })}
                />
              </div>

              <div className="mic-device-row">
                <div className="ricon">
                  <Ms name="settings_voice" />
                </div>
                <div style={{ position: "relative", flex: 1, minWidth: 0 }}>
                  <button className="select mic-device-select" onClick={() => setDeviceOpen((o) => !o)}>
                    <span className="mic-device-name">{deviceLabel}</span>
                    <Ms name="expand_more" />
                  </button>
                  <Popover
                    open={deviceOpen}
                    onClose={() => setDeviceOpen(false)}
                    side="bottom"
                    align="start"
                  >
                    <MenuItem
                      icon="mic"
                      selected={micConfig.input_device === null}
                      onClick={() => {
                        void setMicConfig({ input_device: null });
                        setDeviceOpen(false);
                      }}
                    >
                      System default
                    </MenuItem>
                    {inputDevices.map((d) => (
                      <MenuItem
                        key={d.name}
                        icon="mic"
                        selected={d.name === micConfig.input_device}
                        onClick={() => {
                          void setMicConfig({ input_device: d.name });
                          setDeviceOpen(false);
                        }}
                      >
                        {d.description}
                      </MenuItem>
                    ))}
                  </Popover>
                </div>
                <button
                  className={"sbtn" + (micConfig.muted ? " on-mute" : "")}
                  style={{ width: 34, flex: "0 0 34px" }}
                  title={micConfig.muted ? "Unmute mic" : "Mute mic"}
                  onClick={() => void setMicConfig({ muted: !micConfig.muted })}
                >
                  <Ms name={micConfig.muted ? "mic_off" : "mic"} style={{ fontSize: 18 }} />
                </button>
                <button
                  className={"sbtn" + (listening ? " on-mon" : "")}
                  style={{ width: 34, flex: "0 0 34px" }}
                  title="Listen to yourself - hear the processed mic to tune the chain (wear headphones)"
                  onClick={() => void toggleMonitor(MIC_LEVEL_KEY)}
                >
                  <Ms name="headphones" style={{ fontSize: 18 }} />
                </button>
              </div>


              <MicLevel />

              <div className="mic-gain-row">
                <span className="mic-gain-label">Gain</span>
                <HSlider
                  value={micConfig.gain_percent}
                  max={MAX_MIC_GAIN}
                  onChange={(v) => void setMicConfig({ gain_percent: v })}
                />
              </div>
            </div>

            <div className="section-label">Processing</div>
            <div className="card" style={{ padding: "var(--sp-2)" }}>
              <ToggleRow
                icon="noise_control_off"
                title="Noise gate"
                sub="Cuts the noise floor between words"
                on={micConfig.gate_enabled}
                onToggle={() => void setMicConfig({ gate_enabled: !micConfig.gate_enabled })}
              />
              {micConfig.gate_enabled && (
                <DspSlider
                  label="Threshold"
                  min={-80}
                  max={-10}
                  step={1}
                  unit=" dB"
                  value={micConfig.gate_threshold_db}
                  defaultValue={MIC_DSP_DEFAULTS.gate_threshold_db}
                  onChange={(v) => void setMicConfig({ gate_threshold_db: v })}
                />
              )}
              <ToggleRow
                icon="compress"
                title="Compressor"
                sub="Evens out loud peaks and quiet speech"
                on={micConfig.comp_enabled}
                onToggle={() => void setMicConfig({ comp_enabled: !micConfig.comp_enabled })}
              />
              {micConfig.comp_enabled && (
                <>
                  <DspSlider
                    label="Threshold"
                    min={-60}
                    max={0}
                    step={1}
                    unit=" dB"
                    value={micConfig.comp_threshold_db}
                    defaultValue={MIC_DSP_DEFAULTS.comp_threshold_db}
                    onChange={(v) => void setMicConfig({ comp_threshold_db: v })}
                  />
                  <DspSlider
                    label="Ratio"
                    min={1}
                    max={10}
                    step={0.5}
                    unit=":1"
                    value={micConfig.comp_ratio}
                    defaultValue={MIC_DSP_DEFAULTS.comp_ratio}
                    onChange={(v) => void setMicConfig({ comp_ratio: v })}
                  />
                </>
              )}
              <ToggleRow
                icon="vertical_align_center"
                title="Limiter"
                sub="Hard ceiling - nothing clips downstream"
                on={micConfig.limiter_enabled}
                onToggle={() =>
                  void setMicConfig({ limiter_enabled: !micConfig.limiter_enabled })
                }
              />
              {micConfig.limiter_enabled && (
                <DspSlider
                  label="Ceiling"
                  min={-12}
                  max={0}
                  step={0.5}
                  unit=" dB"
                  value={micConfig.limiter_ceiling_db}
                  defaultValue={MIC_DSP_DEFAULTS.limiter_ceiling_db}
                  onChange={(v) => void setMicConfig({ limiter_ceiling_db: v })}
                />
              )}
            </div>

            <div className="mic-tip">
              <Ms name="lightbulb" />
              <span>
                Tip: running{" "}
                <a
                  href="https://github.com/noisetorch/NoiseTorch"
                  target="_blank"
                  rel="noreferrer"
                >
                  NoiseTorch
                </a>{" "}
                in front of Sink removes background noise before this chain -
                pick its virtual mic as the Input above and the gate gets a
                much cleaner signal to work with.
              </span>
            </div>
        </div>
      </div>
    </div>
  );
}
