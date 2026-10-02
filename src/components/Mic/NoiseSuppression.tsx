import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { NoiseEngineStatus, NoiseSuppression } from "../../types";
import { ConfirmModal } from "../ConfirmModal";
import { Ms } from "../Icons";

const megabytes = (bytes: number) => Math.round(bytes / 1_000_000);

// Measured on a fast desktop CPU (Ryzen 9 9950X3D); slower machines use more.
const LIGHT_COST = "~0.3% of a core and 10 ms of delay";
const STRONG_COST = "~10% of a core and 150 MB of RAM";

/** What the subtitle says: plain text, or "Using <engine link>". */
export interface Subtitle {
  text: string;
  warn?: boolean;
  link?: { label: string; mode: NoiseSuppression; after?: string };
  /** Offer to delete the download: only beside the engine it belongs to. */
  deletable?: boolean;
}

export function noiseSubtitle(
  mode: NoiseSuppression,
  status: NoiseEngineStatus | null,
  progress: [number, number] | null,
  error: string | null,
): Subtitle {
  if (progress) {
    return {
      text: `Downloading DeepFilterNet - ${megabytes(progress[0])} of ${megabytes(progress[1])} MB`,
    };
  }
  if (error) return { text: `${error} - pick Strong to try again`, warn: true };
  if (mode === "strong" && status?.state === "failed") {
    return { text: "Strong stopped, using Light - pick Strong to retry", warn: true };
  }
  if (mode === "strong") {
    return {
      text: "",
      link: {
        label: `DeepFilterNet ${status?.version ?? ""}`.trim(),
        mode: "strong",
        after: status?.installed === "system" ? " (system install)" : undefined,
      },
      deletable: status?.installed === "downloaded",
    };
  }
  if (mode === "light") {
    return { text: "", link: { label: "RNNoise", mode: "light", after: " (built in)" } };
  }
  return { text: "Removes fans, hum and room noise behind your voice" };
}

/** Hover text for each mode button. */
export function modeTooltip(mode: NoiseSuppression, status: NoiseEngineStatus | null): string {
  if (mode === "light") return `RNNoise, built in. Depending on your specs, ${LIGHT_COST}.`;
  if (mode === "strong") {
    // Only once the status is known: before that the size would read "0 MB".
    const first =
      status && !status.installed
        ? ` ${megabytes(status.download_bytes)} MB download on first use.`
        : "";
    return `DeepFilterNet. Depending on your specs, ${STRONG_COST}.${first}`;
  }
  return "No noise suppression";
}

/** What clicking a mode button does. */
export function pickAction(
  next: NoiseSuppression,
  mode: NoiseSuppression,
  status: NoiseEngineStatus | null,
): "set" | "retry" | "confirm-download" {
  if (next !== "strong") return "set";
  if (mode === "strong" && status?.state === "failed") return "retry";
  return status?.installed ? "set" : "confirm-download";
}

const MODES: { mode: NoiseSuppression; label: string }[] = [
  { mode: "off", label: "Off" },
  { mode: "light", label: "Light" },
  { mode: "strong", label: "Strong" },
];

function EngineLink({ mode, children }: Readonly<{ mode: NoiseSuppression; children: string }>) {
  return (
    <button
      type="button"
      className="ns-link"
      title="Open the project page"
      onClick={() => void invoke("open_noise_engine_page", { mode })}
    >
      {children}
    </button>
  );
}

export function NoiseSuppressionRow({
  mode,
  onMode,
}: Readonly<{
  mode: NoiseSuppression;
  onMode: (mode: NoiseSuppression) => void;
}>) {
  const [status, setStatus] = useState<NoiseEngineStatus | null>(null);
  const [progress, setProgress] = useState<[number, number] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<"download" | "delete" | null>(null);
  // Async work outlives a screen switch: results land only while mounted,
  // only the newest status request wins, and one action runs at a time.
  const mounted = useRef(true);
  const latest = useRef(0);
  const busy = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    const request = ++latest.current;
    try {
      const s = await invoke<NoiseEngineStatus>("get_noise_engine");
      if (mounted.current && request === latest.current) setStatus(s);
    } catch {
      // A failed poll keeps the last known status; the next one retries.
    }
  }, []);

  const run = async (action: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true;
    setError(null);
    try {
      await action();
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      busy.current = false;
      if (mounted.current) void refresh();
    }
  };

  // While Strong is on, keep watching it: the engine can die on a slow CPU.
  useEffect(() => {
    void refresh();
    if (mode !== "strong") return;
    const id = setInterval(() => void refresh(), 2000);
    return () => clearInterval(id);
  }, [mode, refresh]);

  useEffect(() => {
    const unlisten = listen<[number, number]>("noise-engine-progress", (e) =>
      setProgress(e.payload),
    );
    return () => void unlisten.then((stop) => stop());
  }, []);

  const pick = (next: NoiseSuppression) => {
    setError(null);
    switch (pickAction(next, mode, status)) {
      case "set":
        onMode(next);
        break;
      case "retry":
        void run(() => invoke("retry_noise_engine"));
        break;
      case "confirm-download":
        setConfirm("download");
    }
  };

  const download = () =>
    run(async () => {
      setProgress([0, status?.download_bytes ?? 0]);
      try {
        await invoke("download_noise_engine");
        onMode("strong");
      } finally {
        if (mounted.current) setProgress(null);
      }
    });

  // Delete first: if it fails, Strong keeps running on the file it has.
  const remove = () =>
    run(async () => {
      await invoke("remove_noise_engine");
      if (mode === "strong") onMode("light");
    });

  const sub = noiseSubtitle(mode, status, progress, error);
  const size = `${megabytes(status?.download_bytes ?? 0)} MB`;

  return (
    <>
      <div className="row">
        <div className="ricon">
          <Ms name="noise_aware" />
        </div>
        <div className="rmain">
          <div className="rtitle">Noise suppression</div>
          <div className={"rsub" + (sub.warn ? " ns-warn" : "")}>
            {sub.link ? (
              <>
                Using <EngineLink mode={sub.link.mode}>{sub.link.label}</EngineLink>
                {sub.link.after}
                {sub.deletable && (
                  <>
                    {" · "}
                    <button
                      type="button"
                      className="ns-link ns-link-quiet"
                      disabled={progress !== null}
                      onClick={() => setConfirm("delete")}
                    >
                      Delete download ({size})
                    </button>
                  </>
                )}
              </>
            ) : (
              sub.text
            )}
          </div>
        </div>
        <div className="seg" role="radiogroup" aria-label="Noise suppression">
          {MODES.filter((m) => m.mode !== "strong" || status?.supported).map((m) => (
            <button
              key={m.mode}
              type="button"
              role="radio"
              aria-checked={mode === m.mode}
              className={"seg-btn" + (mode === m.mode ? " active" : "")}
              title={modeTooltip(m.mode, status)}
              disabled={progress !== null}
              onClick={() => pick(m.mode)}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>

      <ConfirmModal
        open={confirm === "download"}
        onClose={() => setConfirm(null)}
        title="Download DeepFilterNet?"
        confirmLabel="Download"
        tone="primary"
        onConfirm={() => void download()}
      >
        A one-time {size} download from{" "}
        <EngineLink mode="strong">github.com/Rikorose/DeepFilterNet</EngineLink>. Depending on your
        specs, it can use {STRONG_COST}.
      </ConfirmModal>
      <ConfirmModal
        open={confirm === "delete"}
        onClose={() => setConfirm(null)}
        title="Delete the DeepFilterNet model?"
        confirmLabel="Delete model"
        onConfirm={() => void remove()}
      >
        This frees {size}. Strong switches to Light, and you can download the model again any time.
      </ConfirmModal>
    </>
  );
}
