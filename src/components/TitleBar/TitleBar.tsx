import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMixerStore } from "../../store/mixer";
import { Ms, SinkMark } from "../Icons";
import { BalanceBar } from "../MixerBoard/BalanceBar";
import { ProfileMenu } from "./ProfileMenu";

/**
 * Frameless headerbar: brand, current screen, engine status, window
 * controls. The close button triggers the normal close-requested flow,
 * which the Rust side intercepts to hide to tray.
 */
export function TitleBar({ screen }: Readonly<{ screen: string }>) {
  const win = getCurrentWindow();
  const error = useMixerStore((s) => s.error);
  const initialized = useMixerStore((s) => s.initialized);

  let status = "Starting…";
  if (error) status = "Engine error";
  else if (initialized) status = "Engine running";

  return (
    <header data-tauri-drag-region className="headerbar">
      <div data-tauri-drag-region className="hb-brand">
        <div className="hb-logo">
          <SinkMark />
        </div>
        <div data-tauri-drag-region className="hb-title">
          Sink
        </div>
      </div>
      <div data-tauri-drag-region className="hb-sub">
        {screen}
      </div>
      <div data-tauri-drag-region className="hb-spacer" />
      <BalanceBar />
      <ProfileMenu />
      <div className={"hb-status" + (error ? " err" : "")}>
        <span className="dot" />
        {status}
      </div>
      <div className="wctl">
        <button type="button" className="wbtn" aria-label="Minimize" onClick={() => void win.minimize()}>
          <Ms name="remove" />
        </button>
        <button
          type="button"
          className="wbtn"
          aria-label="Maximize"
          onClick={() => void win.toggleMaximize()}
        >
          <Ms name="crop_square" style={{ fontSize: 13 }} />
        </button>
        <button
          type="button"
          className="wbtn close"
          aria-label="Close (hide to tray)"
          title="Hides to tray - quit from the tray menu"
          onClick={() => void win.close()}
        >
          <Ms name="close" />
        </button>
      </div>
    </header>
  );
}
