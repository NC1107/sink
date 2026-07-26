import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { TitleBar } from "./components/TitleBar/TitleBar";
import { MixerBoard } from "./components/MixerBoard/MixerBoard";
import { AppList } from "./components/AppList/AppList";
import { MicScreen } from "./components/Mic/MicScreen";
import { HeadsetScreen } from "./components/Headset/HeadsetScreen";
import { OledScreen } from "./components/Oled/OledScreen";
import { MouseScreen } from "./components/Mouse/MouseScreen";
import { OnboardingModal } from "./components/Onboarding/OnboardingModal";
import { SettingsScreen } from "./components/Settings/SettingsScreen";
import { Ms } from "./components/Icons";
import { Tooltip } from "./components/Tooltip";
import { useAudio } from "./hooks/useAudio";
import { useMixerStore } from "./store/mixer";

const NAV = [
  { id: "mixer", icon: "graphic_eq", label: "Mixer" },
  { id: "apps", icon: "grid_view", label: "Apps" },
  { id: "mic", icon: "mic", label: "Mic" },
  { id: "headset", icon: "headphones", label: "Headset" },
  { id: "oled", icon: "tv_gen", label: "OLED" },
  { id: "mouse", icon: "mouse", label: "Mouse" },
] as const;

type NavId = (typeof NAV)[number]["id"] | "settings";

export default function App() {
  useAudio();
  const [nav, setNav] = useState<NavId>("mixer");
  const [version, setVersion] = useState("");
  const error = useMixerStore((s) => s.error);
  const clearError = useMixerStore((s) => s.clearError);

  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);

  const current =
    nav === "settings"
      ? { label: "Settings" }
      : (NAV.find((n) => n.id === nav) ?? NAV[0]);

  let screen;
  if (nav === "mixer") screen = <MixerBoard />;
  else if (nav === "apps") screen = <AppList />;
  else if (nav === "mic") screen = <MicScreen />;
  else if (nav === "headset") screen = <HeadsetScreen />;
  else if (nav === "oled") screen = <OledScreen />;
  else if (nav === "mouse") screen = <MouseScreen />;
  else screen = <SettingsScreen />;

  return (
    <div className="window">
      <TitleBar screen={current.label} />

      {error && (
        <div className="error-banner" role="alert">
          <span className="error-banner-msg">
            <strong>Audio error:</strong> {error}
          </span>
          <button
            type="button"
            className="error-banner-x"
            aria-label="Dismiss error"
            title="Dismiss"
            onClick={clearError}
          >
            <Ms name="close" style={{ fontSize: 16 }} />
          </button>
        </div>
      )}

      <div className="body">
        <nav className="rail">
          {NAV.map((n) => (
            <button
              type="button"
              key={n.id}
              className={"nav-item" + (n.id === nav ? " active" : "")}
              onClick={() => setNav(n.id)}
            >
              <Ms name={n.icon} />
              <span className="nav-label">{n.label}</span>
            </button>
          ))}
          <div className="rail-spacer" />
          <button
            type="button"
            className={"nav-item" + (nav === "settings" ? " active" : "")}
            onClick={() => setNav("settings")}
          >
            <Ms name="settings" />
            <span className="nav-label">Settings</span>
          </button>
          {version && <div className="rail-version">v{version}</div>}
        </nav>

        {screen}
      </div>

      <OnboardingModal />
      <Tooltip />
    </div>
  );
}
