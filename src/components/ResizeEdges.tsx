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
 * the drag over itself. */
export function ResizeEdges() {
  return (
    <>
      {EDGES.map((dir) => (
        <div
          key={dir}
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
