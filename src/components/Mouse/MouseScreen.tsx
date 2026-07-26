import { useEffect } from "react";
import { Ms } from "../Icons";
import { useMouse, ZONE_LABELS } from "../../store/mouse";

/** #rrggbb <-> [r,g,b] for the native colour inputs. */
function toHex([r, g, b]: [number, number, number]) {
  return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}
function fromHex(hex: string): [number, number, number] {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

export function MouseScreen() {
  const m = useMouse();
  const cfg = m.config;

  useEffect(() => {
    void m.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!m.connected) {
    return (
      <div className="content narrow">
        <div className="screen-head">
          <h1>Mouse</h1>
        </div>
        <div className="hs-empty">
          <Ms name="mouse" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>No SteelSeries mouse connected.</p>
          <span className="hs-empty-sub">
            Plug in an Aerox 9 Wireless (or its 2.4&nbsp;GHz dongle). If it is
            connected, make sure your user can access <code>/dev/hidraw*</code> —
            the bundled udev rule handles this.
          </span>
        </div>
      </div>
    );
  }

  const battery = m.status.battery_percent;
  const tone = battery == null ? "ok" : battery <= 15 ? "low" : battery <= 40 ? "mid" : "ok";

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>{m.status.model ?? "Mouse"}</h1>
        <div className="screen-head-actions">
          <span className="tag live">
            <Ms name={m.status.wireless ? "wifi" : "usb"} style={{ fontSize: 11 }} />
            {m.status.wireless ? "Wireless" : "Wired"}
          </span>
        </div>
      </div>

      <div className="hs-body">
        {m.error && (
          <div className="error-banner" style={{ borderRadius: 8 }}>
            {m.error}
          </div>
        )}

        {/* ---- battery ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="battery_full" style={{ fontSize: 18 }} />
            <h2>Battery</h2>
          </div>
          {battery == null ? (
            <p className="hs-hint">
              Waiting for a reading — a sleeping mouse answers on the next poll.
            </p>
          ) : (
            <div className="hs-batt">
              <span className="hs-batt-label">
                {m.status.charging ? "Charging" : "Level"}
              </span>
              <div className="hs-batt-bar">
                <div className={"hs-batt-fill " + tone} style={{ width: `${battery}%` }} />
              </div>
              <span className="hs-batt-pct">{battery}%</span>
            </div>
          )}
        </section>

        {/* ---- sensor ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="my_location" style={{ fontSize: 18 }} />
            <h2>Sensor</h2>
          </div>

          <div className="hs-slider-top">
            <span>DPI presets</span>
            <span className="hs-slider-val">
              {cfg.dpi_presets[cfg.dpi_selected] ?? "?"} active
            </span>
          </div>
          <div className="hs-eq-presets">
            {cfg.dpi_presets.map((dpi, i) => (
              <button
                key={`${dpi}-${i}`}
                type="button"
                className={"hs-chip" + (i === cfg.dpi_selected ? " primary" : "")}
                onClick={() => void m.setDpi(cfg.dpi_presets, i)}
              >
                {dpi}
              </button>
            ))}
          </div>

          {/* Editing the active preset's value. */}
          <label className="hs-slider">
            <div className="hs-slider-top">
              <span>Active preset value</span>
              <span className="hs-slider-val">
                {cfg.dpi_presets[cfg.dpi_selected]} DPI
              </span>
            </div>
            <input
              type="range"
              min={0}
              max={Math.max(0, m.dpiSteps.length - 1)}
              value={Math.max(
                0,
                m.dpiSteps.indexOf(cfg.dpi_presets[cfg.dpi_selected] ?? 800),
              )}
              onChange={(e) => {
                const dpi = m.dpiSteps[Number(e.target.value)];
                const next = cfg.dpi_presets.slice();
                next[cfg.dpi_selected] = dpi;
                void m.setDpi(next, cfg.dpi_selected);
              }}
            />
          </label>

          <div className="hs-row">
            <span>Polling rate</span>
            <div className="hs-seg">
              {m.pollingRates.map((hz) => (
                <button
                  key={hz}
                  type="button"
                  className={"hs-seg-btn" + (hz === cfg.polling_hz ? " active" : "")}
                  onClick={() => void m.setPolling(hz)}
                >
                  {hz}
                </button>
              ))}
            </div>
          </div>
        </section>

        {/* ---- lighting ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="lightbulb" style={{ fontSize: 18 }} />
            <h2>Lighting</h2>
          </div>
          <div className="hs-oled-actions">
            <button
              type="button"
              className={"hs-chip" + (cfg.rainbow ? " primary" : "")}
              onClick={() => void m.setRainbow()}
            >
              <Ms name="looks" style={{ fontSize: 14 }} /> Rainbow
            </button>
          </div>
          {cfg.zones.map((zone, i) => (
            <div className="hs-row" key={i}>
              <span>{ZONE_LABELS[i] ?? `Zone ${i + 1}`}</span>
              <input
                type="color"
                className="hs-color"
                value={toHex(zone)}
                onChange={(e) => void m.setZoneColor(i, fromHex(e.target.value))}
              />
            </div>
          ))}
          <div className="hs-row">
            <span>
              Reactive click flash
              <span className="hs-hint" style={{ display: "block" }}>
                Flashes a colour on every click
              </span>
            </span>
            <div className="hs-oled-actions">
              <input
                type="color"
                className="hs-color"
                value={toHex(cfg.reactive ?? [255, 255, 255])}
                onChange={(e) => void m.setReactive(fromHex(e.target.value))}
              />
              <button
                type="button"
                className="hs-chip"
                onClick={() => void m.setReactive(null)}
              >
                Off
              </button>
            </div>
          </div>
        </section>

        {/* ---- power ---- */}
        <section className="hs-card">
          <div className="hs-card-head">
            <Ms name="battery_saver" style={{ fontSize: 18 }} />
            <h2>Power saving</h2>
          </div>
          <label className="hs-slider">
            <div className="hs-slider-top">
              <span>Sleep after</span>
              <span className="hs-slider-val">
                {cfg.sleep_minutes === 0 ? "Never" : `${cfg.sleep_minutes} min`}
              </span>
            </div>
            <input
              type="range"
              min={0}
              max={20}
              value={cfg.sleep_minutes}
              onChange={(e) => void m.setSleep(Number(e.target.value))}
            />
          </label>
          <label className="hs-slider">
            <div className="hs-slider-top">
              <span>Dim lighting after</span>
              <span className="hs-slider-val">
                {cfg.dim_seconds === 0 ? "Never" : `${cfg.dim_seconds} s`}
              </span>
            </div>
            <input
              type="range"
              min={0}
              max={1200}
              step={10}
              value={cfg.dim_seconds}
              onChange={(e) => void m.setDim(Number(e.target.value))}
            />
          </label>
        </section>
      </div>
    </div>
  );
}
