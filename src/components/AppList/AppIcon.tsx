import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Ms } from "../Icons";

interface AppIconProps {
  /** Resolved absolute icon path from the backend's desktop-entry
   * resolver; null when nothing matched. */
  iconPath: string | null;
}

/** App icon via the asset protocol, generic glyph fallback. */
export function AppIcon({ iconPath }: Readonly<AppIconProps>) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [iconPath]);

  if (iconPath && !failed) {
    return <img src={convertFileSrc(iconPath)} alt="" onError={() => setFailed(true)} />;
  }
  return <Ms name="graphic_eq" />;
}
