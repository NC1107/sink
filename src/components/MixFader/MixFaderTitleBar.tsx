import { getCurrentWindow } from "@tauri-apps/api/window";
import { Ms, SinkMark } from "../Icons";

/**
 * Frameless headerbar for the mix fader popout - same chrome as the main
 * window (brand, drag region, window controls) so the popout doesn't look
 * like a different app. Close is a real close here (see the Rust-side
 * `on_window_event` guard), not the main window's hide-to-tray.
 */
export function MixFaderTitleBar({ label }: Readonly<{ label: string }>) {
  const win = getCurrentWindow();

  return (
    <header data-tauri-drag-region className="headerbar">
      <div data-tauri-drag-region className="hb-brand">
        <div className="hb-logo">
          <SinkMark />
        </div>
        <div data-tauri-drag-region className="hb-title">
          {label}
        </div>
      </div>
      <div data-tauri-drag-region className="hb-sub">
        Levels
      </div>
      <div data-tauri-drag-region className="hb-spacer" />
      <div className="wctl">
        <button
          type="button"
          className="wbtn"
          aria-label="Minimize"
          onClick={() => void win.minimize()}
        >
          <Ms name="remove" />
        </button>
        <button type="button" className="wbtn close" aria-label="Close" onClick={() => void win.close()}>
          <Ms name="close" />
        </button>
      </div>
    </header>
  );
}
