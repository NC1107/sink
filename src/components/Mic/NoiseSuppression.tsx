import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { NoiseEngineStatus, NoiseSuppression } from "../../types";
import { ToggleRow } from "../Toggle";

const megabytes = (bytes: number) => `${Math.round(bytes / 1_000_000)} MB`;

const HINTS: Record<Exclude<NoiseSuppression, "off">, string> = {
  light: "Built in, almost no CPU",
  strong: "DeepFilterNet - better on keyboards and loud fans",
};

/** Which notice the Strong engine needs under the mode picker, if any. */
export function engineNotice(
  mode: NoiseSuppression,
  status: NoiseEngineStatus | null,
  offering: boolean,
): "offer" | "failed" | null {
  if (!status) return null;
  if (offering || (mode === "strong" && status.state === "missing")) return "offer";
  if (mode === "strong" && status.state === "failed") return "failed";
  return null;
}

export function NoiseSuppressionRow({
  mode,
  onMode,
}: Readonly<{
  mode: NoiseSuppression;
  onMode: (mode: NoiseSuppression) => void;
}>) {
  const [status, setStatus] = useState<NoiseEngineStatus | null>(null);
  const [offering, setOffering] = useState(false);
  const [progress, setProgress] = useState<[number, number] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    void invoke<NoiseEngineStatus>("get_noise_engine").then(setStatus);
  }, []);

  // While Strong is on, keep watching it: the engine can die on a slow CPU.
  useEffect(() => {
    refresh();
    if (mode !== "strong") return;
    const id = setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, [mode, refresh]);

  useEffect(() => {
    const unlisten = listen<[number, number]>("noise-engine-progress", (e) =>
      setProgress(e.payload),
    );
    return () => void unlisten.then((stop) => stop());
  }, []);

  const pickStrong = () => {
    if (status?.installed) onMode("strong");
    else setOffering(true);
  };

  const download = async () => {
    setError(null);
    setProgress([0, status?.download_bytes ?? 0]);
    try {
      await invoke("download_noise_engine");
      setOffering(false);
      onMode("strong");
    } catch (e) {
      setError(String(e));
    } finally {
      setProgress(null);
      refresh();
    }
  };

  const retry = async () => {
    await invoke("retry_noise_engine").catch((e: unknown) => setError(String(e)));
    refresh();
  };

  const remove = async () => {
    if (mode === "strong") onMode("light");
    await invoke("remove_noise_engine").catch((e: unknown) => setError(String(e)));
    refresh();
  };

  const notice = engineNotice(mode, status, offering);
  const on = mode !== "off";

  return (
    <>
      <ToggleRow
        icon="noise_aware"
        title="Noise suppression"
        sub="Removes fans, hum and room noise behind your voice"
        on={on}
        onToggle={() => onMode(on ? "off" : "light")}
      />
      {on && (
        <div className="dsp-row">
          <span className="dsp-label">Mode</span>
          <div className="seg" role="radiogroup" aria-label="Noise suppression mode">
            <button
              type="button"
              role="radio"
              aria-checked={mode === "light"}
              className={"seg-btn" + (mode === "light" ? " active" : "")}
              onClick={() => {
                setOffering(false);
                onMode("light");
              }}
            >
              Light
            </button>
            {status?.supported && (
              <button
                type="button"
                role="radio"
                aria-checked={mode === "strong"}
                className={"seg-btn" + (mode === "strong" ? " active" : "")}
                onClick={pickStrong}
              >
                Strong
              </button>
            )}
          </div>
          <span className="ns-hint">{HINTS[mode]}</span>
        </div>
      )}
      {on && notice === "offer" && (
        <div className="dsp-row ns-panel">
          {progress ? (
            <>
              <div className="ns-progress">
                <div
                  className="ns-progress-fill"
                  style={{
                    width: (progress[1] ? (progress[0] / progress[1]) * 100 : 0).toFixed(1) + "%",
                  }}
                />
              </div>
              <span className="ns-hint">
                {megabytes(progress[0])} / {megabytes(progress[1])}
              </span>
            </>
          ) : (
            <>
              <span className="ns-text">
                {error ??
                  `Strong runs DeepFilterNet, a one-time ${megabytes(status?.download_bytes ?? 0)} download. It uses more CPU than Light.`}
              </span>
              <button type="button" className="modal-btn ns-btn" onClick={() => setOffering(false)}>
                Cancel
              </button>
              <button
                type="button"
                className="modal-btn primary ns-btn"
                onClick={() => void download()}
              >
                {error ? "Retry" : "Download"}
              </button>
            </>
          )}
        </div>
      )}
      {on && notice === "failed" && (
        <div className="dsp-row ns-panel">
          <span className="ns-text ns-warn">
            Strong stopped, so Light is covering for it. Your CPU may be too slow for it.
          </span>
          <button type="button" className="modal-btn ns-btn" onClick={() => void retry()}>
            Retry
          </button>
        </div>
      )}
      {on && !notice && status?.installed === "downloaded" && (
        <div className="dsp-row ns-panel">
          <button type="button" className="ns-link" onClick={() => void remove()}>
            Remove DeepFilterNet download ({megabytes(status.download_bytes)})
          </button>
        </div>
      )}
    </>
  );
}
