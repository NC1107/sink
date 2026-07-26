import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Ms } from "../Icons";
import { useHeadset } from "../../store/headset";

const MEDIA_EXTS = [
  "png", "jpg", "jpeg", "bmp", "webp", "gif",
  "mp4", "mkv", "webm", "mov", "avi", "m4v",
];

// Icons per clip category for the gallery.
const CATEGORY_ICONS: Record<string, string> = {
  Japan: "temple_buddhist",
  Effects: "auto_awesome",
  Demos: "sports_esports",
};

/** Rough 128x64 preview of static text (real rendering happens on-device). */
const PREVIEW_SLOTS = ["l0", "l1", "l2", "l3"];

function Preview({ lines }: Readonly<{ lines: string[] }>) {
  return (
    <div className="oled-preview" aria-hidden>
      <div className="oled-preview-inner">
        {PREVIEW_SLOTS.map((slot, i) => (
          <div key={slot} className="oled-preview-line">
            {lines[i] || " "}
          </div>
        ))}
      </div>
    </div>
  );
}

export function OledScreen() {
  const h = useHeadset();
  const [text, setText] = useState("SINK");
  const [brightness, setBrightness] = useState(8);
  const [loop, setLoop] = useState(true);
  const [notifyMs, setNotifyMs] = useState(4000);
  const [active, setActive] = useState<string | null>(null);
  const [rotation, setRotation] = useState<string[]>([]);
  const [rotateSecs, setRotateSecs] = useState(15);

  const toggleRotation = (id: string, on: boolean) =>
    setRotation((r) => (on ? [...r, id] : r.filter((x) => x !== id)));

  useEffect(() => {
    void h.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!h.connected) {
    return (
      <div className="content narrow">
        <div className="screen-head">
          <h1>OLED</h1>
        </div>
        <div className="hs-empty">
          <Ms name="tv_gen" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>No base station detected.</p>
          <span className="hs-empty-sub">
            The OLED lives on the Arctis Nova Pro Wireless base station — connect
            it over USB to drive the display.
          </span>
        </div>
      </div>
    );
  }

  // A headset is connected, but this model has no display we can drive
  // (e.g. the pre-Nova Arctis Pro Wireless, whose panel isn't documented).
  if (!h.hasOled) {
    return (
      <div className="content narrow">
        <div className="screen-head">
          <h1>OLED</h1>
        </div>
        <div className="hs-empty">
          <Ms name="tv_off" style={{ fontSize: 46, opacity: 0.5 }} />
          <p>Display not supported on this device.</p>
          <span className="hs-empty-sub">
            {h.model ?? "The connected device"} doesn't expose a display Sink can
            drive. Screen control currently works on the Arctis Nova Pro Wireless
            base station.
          </span>
        </div>
      </div>
    );
  }

  const lines = text.split("\n");

  async function pickMedia() {
    const path = await open({
      multiple: false,
      filters: [{ name: "Media", extensions: MEDIA_EXTS }],
    });
    if (typeof path === "string") await h.oledMedia(path, loop);
  }

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>OLED</h1>
        <div className="screen-head-actions">
          <span className="tag live">
            <Ms name="tv_gen" style={{ fontSize: 11 }} /> 128×64
          </span>
        </div>
      </div>

      <div className="hs-body">
      <Preview lines={lines} />

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="brightness_medium" style={{ fontSize: 18 }} />
          <h2>Brightness</h2>
        </div>
        <input
          type="range"
          min={1}
          max={10}
          value={brightness}
          onChange={(e) => {
            const v = Number(e.target.value);
            setBrightness(v);
            h.oledBrightness(v);
          }}
        />
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="text_fields" style={{ fontSize: 18 }} />
          <h2>Text</h2>
        </div>
        <textarea
          className="oled-text"
          rows={4}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="One line per row (up to 4)…"
        />
        <div className="hs-oled-actions">
          <button type="button" className="hs-chip primary" onClick={() => void h.oledText(lines)}>
            <Ms name="send" style={{ fontSize: 14 }} /> Show text
          </button>
          <button type="button" className="hs-chip" onClick={() => void h.oledNotify(lines, notifyMs)}>
            <Ms name="notifications" style={{ fontSize: 14 }} /> Notify
          </button>
          <label className="hs-inline-num">
            <input
              type="number"
              min={0.5}
              max={60}
              step={0.5}
              value={notifyMs / 1000}
              onChange={(e) => setNotifyMs(Math.round(Number(e.target.value) * 1000))}
            />
            {" "}s
          </label>
        </div>
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="animation" style={{ fontSize: 18 }} />
          <h2>Built-in clips</h2>
        </div>
        {Array.from(new Set(h.clips.map((c) => c.category))).map((cat) => (
          <div key={cat}>
            <div className="hs-clip-cat">{cat}</div>
            <div className="hs-clip-grid">
              {h.clips
                .filter((c) => c.category === cat)
                .map((c) => (
                  <button
                    key={c.id}
                    type="button"
                    className="hs-clip"
                    onClick={() => void h.oledClip(c.id)}
                  >
                    <Ms name={CATEGORY_ICONS[cat] ?? "movie"} style={{ fontSize: 20 }} />
                    <span>{c.label}</span>
                  </button>
                ))}
            </div>
          </div>
        ))}
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="upload" style={{ fontSize: 18 }} />
          <h2>Upload image · GIF · video</h2>
        </div>
        <div className="hs-oled-actions">
          <button type="button" className="hs-chip primary" onClick={() => void pickMedia()}>
            <Ms name="folder_open" style={{ fontSize: 14 }} /> Choose file…
          </button>
          <label className="hs-check">
            <input type="checkbox" checked={loop} onChange={(e) => setLoop(e.target.checked)} />
            {" "}Loop animation
          </label>
        </div>
        <p className="hs-hint">
          Your file is scaled to 128×64 and dithered to 1-bit on the fly.
          {h.videoSupported
            ? " Videos, GIFs and stills all work."
            : " Install ffmpeg to convert videos (GIFs and stills work now)."}
        </p>
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="dashboard" style={{ fontSize: 18 }} />
          <h2>Live modes</h2>
          <div className="hs-card-right">
            <button type="button" className="hs-chip" onClick={() => void h.oledReturnUi()}>
              <Ms name="undo" style={{ fontSize: 14 }} /> SteelSeries UI
            </button>
          </div>
        </div>

        {/* Grouped picker: tap to show, or tick for the rotation set. */}
        {Array.from(new Set(h.modes.map((m) => m.category))).map((cat) => (
          <div key={cat}>
            <div className="hs-clip-cat">{cat}</div>
            <div className="hs-mode-grid">
              {h.modes
                .filter((m) => m.category === cat)
                .map((m) => (
                  <div
                    key={m.id}
                    className={"hs-mode" + (active === m.id ? " active" : "")}
                  >
                    <button
                      type="button"
                      className="hs-mode-btn"
                      onClick={() => {
                        setActive(m.id);
                        void h.oledMode(m.id);
                      }}
                    >
                      {m.label}
                    </button>
                    <label
                      className="hs-mode-tick"
                      title="Include in rotation"
                      aria-label={`Include ${m.label} in rotation`}
                    >
                      <input
                        type="checkbox"
                        checked={rotation.includes(m.id)}
                        onChange={(e) => toggleRotation(m.id, e.target.checked)}
                      />
                    </label>
                  </div>
                ))}
            </div>
          </div>
        ))}
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="autoplay" style={{ fontSize: 18 }} />
          <h2>Automatic</h2>
        </div>
        <div className="hs-oled-actions">
          <button
            type="button"
            className="hs-chip primary"
            disabled={rotation.length === 0}
            onClick={() => {
              setActive("__rotate");
              void h.oledRotate(rotation, rotateSecs);
            }}
          >
            <Ms name="rotate_right" style={{ fontSize: 14 }} />
            Rotate {rotation.length > 0 ? `(${rotation.length})` : ""}
          </button>
          <label className="hs-inline-num">
            every{" "}
            <input
              type="number"
              min={2}
              max={600}
              value={rotateSecs}
              onChange={(e) => setRotateSecs(Number(e.target.value))}
            />
            {" "}s
          </label>
          <button
            type="button"
            className="hs-chip"
            onClick={() => {
              setActive("__auto");
              void h.oledAuto();
            }}
          >
            <Ms name="auto_mode" style={{ fontSize: 14 }} /> Context-aware
          </button>
        </div>
        <p className="hs-hint">
          Tick modes above to build the rotation. Context-aware picks for you:
          music playing shows Now Playing, sound during load shows the VU meters,
          an idle headset falls back to the clock.
        </p>
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="timer" style={{ fontSize: 18 }} />
          <h2>Timer</h2>
        </div>
        <div className="hs-oled-actions">
          {[5, 10, 25, 45].map((min) => (
            <button
              key={min}
              type="button"
              className="hs-chip"
              onClick={() => void h.timerCountdown(min * 60)}
            >
              {min} min
            </button>
          ))}
          <button type="button" className="hs-chip" onClick={() => void h.timerStopwatch()}>
            <Ms name="timer" style={{ fontSize: 14 }} /> Stopwatch
          </button>
          <button type="button" className="hs-chip" onClick={() => void h.timerToggle()}>
            <Ms name="pause" style={{ fontSize: 14 }} /> Pause
          </button>
          <button type="button" className="hs-chip" onClick={() => void h.timerReset()}>
            <Ms name="replay" style={{ fontSize: 14 }} /> Reset
          </button>
        </div>
      </section>

      <section className="hs-card">
        <div className="hs-card-head">
          <Ms name="notifications" style={{ fontSize: 18 }} />
          <h2>Notifications</h2>
        </div>
        <div className="hs-row">
          <span>
            Mirror desktop notifications{" "}
            <span className="hs-hint" style={{ display: "block" }}>
              Shows PC notifications on the OLED for a few seconds
            </span>
          </span>
          <label className="hs-check">
            <input
              type="checkbox"
              checked={h.notifyMirror}
              onChange={(e) => void h.setNotifyMirror(e.target.checked)}
            />
            {h.notifyMirror ? "On" : "Off"}
          </label>
        </div>
        <div className="hs-row">
          <span>
            Show for{" "}
            <span className="hs-hint" style={{ display: "block" }}>
              How long a notification stays before the previous screen returns
            </span>
          </span>
          <label className="hs-inline-num" aria-label="Notification duration in seconds">
            <input
              type="number"
              min={1}
              max={60}
              value={h.notifyDurationSecs}
              onChange={(e) =>
                void h.setNotifyDisplay(
                  Math.min(60, Math.max(1, Math.round(Number(e.target.value)) || 1)),
                  h.notifyScroll,
                )
              }
            />
            {" "}s
          </label>
        </div>
        <div className="hs-row">
          <span>
            Long text{" "}
            <span className="hs-hint" style={{ display: "block" }}>
              Direction to scroll when a notification is too long to fit
            </span>
          </span>
          <div className="hs-seg">
            <button
              type="button"
              className={"hs-seg-btn" + (h.notifyScroll === "vertical" ? " active" : "")}
              onClick={() => void h.setNotifyDisplay(h.notifyDurationSecs, "vertical")}
            >
              Vertical
            </button>
            <button
              type="button"
              className={"hs-seg-btn" + (h.notifyScroll === "horizontal" ? " active" : "")}
              onClick={() => void h.setNotifyDisplay(h.notifyDurationSecs, "horizontal")}
            >
              Horizontal
            </button>
          </div>
        </div>
      </section>
      </div>
    </div>
  );
}
