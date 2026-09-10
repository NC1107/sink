import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Window } from "@tauri-apps/api/window";

type Direction = Parameters<Window["startResizeDragging"]>[0];

const EDGES: readonly Direction[] = [
  "North",
  "South",
  "East",
  "West",
  "NorthEast",
  "NorthWest",
  "SouthEast",
  "SouthWest",
];

/** Invisible grab zones along the window edge. An undecorated window gets
 * no resize border from the compositor on Linux, so the app has to hand
 * the drag over itself. Gone while maximised, where a drag means nothing. */
export function ResizeEdges() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    const refresh = () => void win.isMaximized().then(setMaximized);
    refresh();
    const unlisten = win.onResized(refresh);
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  if (maximized) return null;
  return (
    <>
      {EDGES.map((dir) => (
        <div
          key={dir}
          aria-hidden="true"
          className={`resize-edge resize-${dir.toLowerCase()}`}
          onPointerDown={(e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            void getCurrentWindow().startResizeDragging(dir);
          }}
        />
      ))}
    </>
  );
}
