import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { HotkeyStatus } from "../../types";
import { Ms } from "../Icons";

const ICONS: Record<string, string> = {
  "profile.next": "skip_next",
  "profile.prev": "skip_previous",
  "balance.a": "keyboard_arrow_left",
  "balance.b": "keyboard_arrow_right",
  "balance.center": "vertical_align_center",
};

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "ShiftLeft",
  "ShiftRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
]);

/** An accelerator in the form the X11 grabber parses, from a key event. */
export function acceleratorFrom(e: {
  code: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}): string | null {
  if (MODIFIER_CODES.has(e.code) || !e.code) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Super");
  parts.push(e.code);
  return parts.join("+");
}

export function HotkeysSection({ onError }: Readonly<{ onError: (e: string) => void }>) {
  const [status, setStatus] = useState<HotkeyStatus | null>(null);
  const [capturing, setCapturing] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<HotkeyStatus>("get_hotkeys"));
    } catch (e) {
      onError(String(e));
    }
  }, [onError]);
  useEffect(() => {
    void refresh();
    // The desktop owns the bindings; pick up edits made there, and re-read
    // when the window comes back from its settings.
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    const unlisten = listen("hotkeys-changed", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      void unlisten.then((stop) => stop());
    };
  }, [refresh]);

  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.key === "Escape") {
        setCapturing(null);
        return;
      }
      const trigger = acceleratorFrom(e);
      if (!trigger) return;
      const id = capturing;
      setCapturing(null);
      void invoke("set_hotkey_binding", { id, trigger })
        .then(refresh)
        .catch((err) => onError(String(err)));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, refresh, onError]);

  const setStep = (step: number) => {
    void invoke("set_balance_step", { step })
      .then(refresh)
      .catch((e) => onError(String(e)));
  };
  const configure = () => {
    void invoke("configure_hotkeys")
      .then(refresh)
      .catch((e) => onError(String(e)));
  };

  if (!status) return null;
  return (
    <>
      <div className="section-label">Hotkeys</div>
      <div className="card" style={{ padding: "var(--sp-2)" }}>
        {status.backend === "none" && (
          <div className="row">
            <div className="ricon">
              <Ms name="keyboard" />
            </div>
            <div className="rmain">
              <div className="rtitle">Global hotkeys aren’t available here</div>
              <div className="rsub">
                They need a desktop with the GlobalShortcuts portal (KDE Plasma, GNOME 48, Hyprland) or an X11 session
              </div>
            </div>
          </div>
        )}
        {status.shortcuts.map((s) => (
          <div className="row row-compact" key={s.id}>
            <Ms name={ICONS[s.id] ?? "keyboard"} className="row-compact-icon" />
            <div className="rmain">
              <div className="rtitle">{s.description}</div>
            </div>
            {status.backend === "x11" ? (
              <button type="button" className="kbd kbd-btn" onClick={() => setCapturing(capturing === s.id ? null : s.id)}>
                {capturing === s.id ? "Press keys…" : s.trigger || "Not bound"}
              </button>
            ) : (
              <span className={"kbd" + (s.trigger ? "" : " kbd-unbound")}>{s.trigger || "Not bound"}</span>
            )}
          </div>
        ))}
        {status.backend === "portal" && (
          <div className="row">
            <div className="ricon">
              <Ms name="tune" />
            </div>
            <div className="rmain">
              <div className="rtitle">Key bindings</div>
              <div className="rsub">Kept by your desktop; also under System Settings › Shortcuts</div>
            </div>
            <button type="button" className="modal-btn primary" onClick={configure}>
              Set up hotkeys
            </button>
          </div>
        )}
        <div className="row">
          <div className="ricon">
            <Ms name="swap_horiz" />
          </div>
          <div className="rmain">
            <div className="rtitle">Balance step</div>
            <div className="rsub">Percentage points the balance moves per press</div>
          </div>
          <div className="seg" role="radiogroup" aria-label="Balance step">
            {status.steps.map((v) => (
              <button
                type="button"
                key={v}
                role="radio"
                aria-checked={v === status.balance_step}
                className={"seg-btn" + (v === status.balance_step ? " active" : "")}
                onClick={() => setStep(v)}
              >
                {v}
              </button>
            ))}
          </div>
        </div>
      </div>
    </>
  );
}
