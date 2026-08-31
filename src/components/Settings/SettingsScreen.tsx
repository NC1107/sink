import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { useMixerStore } from "../../store/mixer";
import { useTheme, THEMES } from "../../store/theme";
import type { OutputDevice, VirtualSink } from "../../types";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

interface DefaultDevices {
  output: string | null;
  input: string | null;
}

type LabelStyle = "plain" | "suffix" | "prefix";
type ArctisConnectionState =
  | "disabled"
  | "connected"
  | "disconnected"
  | "permission_denied"
  | "unsupported";

interface ArctisStatus {
  state: ArctisConnectionState;
  detail: string | null;
}

const LABEL_STYLES: { value: LabelStyle; label: string; example: string }[] = [
  { value: "plain", label: "Plain", example: "Game" },
  { value: "suffix", label: "Suffix", example: "Game (Sink)" },
  { value: "prefix", label: "Prefix", example: "Sink · Game" },
];

/** Card row with a device dropdown for picking a system default. */
function DeviceRow({
  icon,
  title,
  sub,
  devices,
  current,
  onPick,
}: Readonly<{
  icon: string;
  title: string;
  /** What this default is used for. */
  sub: string;
  devices: OutputDevice[];
  current: string | null;
  onPick: (name: string) => void;
}>) {
  const [open, setOpen] = useState(false);
  const currentDesc = devices.find((d) => d.name === current)?.description ?? current ?? "-";

  return (
    <div className="row">
      <div className="ricon">
        <Ms name={icon} />
      </div>
      <div className="rmain">
        <div className="rtitle">{title}</div>
        <div className="rsub">{sub}</div>
      </div>
      <div style={{ position: "relative" }}>
        <button type="button" className="select device-select" onClick={() => setOpen((o) => !o)}>
          <span className="device-select-name">{currentDesc}</span>
          <Ms name="expand_more" />
        </button>
        <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
          {devices.map((d) => (
            <MenuItem
              key={d.name}
              icon={icon}
              selected={d.name === current}
              showCheck
              onClick={() => {
                onPick(d.name);
                setOpen(false);
              }}
            >
              {d.description}
            </MenuItem>
          ))}
        </Popover>
      </div>
    </div>
  );
}

function ArctisOutputRow({
  devices,
  current,
  onPick,
}: Readonly<{
  devices: OutputDevice[];
  current: string | null;
  onPick: (name: string | null) => void;
}>) {
  const [open, setOpen] = useState(false);
  const label = current
    ? devices.find((device) => device.name === current)?.description ?? current
    : "Use each channel's output";
  return (
    <div className="row row-sub">
      <div className="ricon"><Ms name="speaker_group" /></div>
      <div className="rmain">
        <div className="rtitle">Headset output</div>
        <div className="rsub">Physical Arctis or Easy Effects; keep Easy Effects' output physical to avoid a loop</div>
      </div>
      <div style={{ position: "relative" }}>
        <button type="button" className="select device-select" onClick={() => setOpen((value) => !value)}>
          <span className="device-select-name">{label}</span>
          <Ms name="expand_more" />
        </button>
        <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
          <MenuItem
            icon="alt_route"
            selected={current === null}
            showCheck
            onClick={() => {
              onPick(null);
              setOpen(false);
            }}
          >
            Use each channel's output
          </MenuItem>
          {devices.map((device) => (
            <MenuItem
              key={device.name}
              icon="speaker"
              selected={device.name === current}
              showCheck
              onClick={() => {
                onPick(device.name);
                setOpen(false);
              }}
            >
              {device.description}
            </MenuItem>
          ))}
        </Popover>
      </div>
    </div>
  );
}

function BalanceChannelsRow({
  channels,
  aName,
  bName,
  onPick,
}: Readonly<{
  channels: VirtualSink[];
  aName: string | null;
  bName: string | null;
  onPick: (a: string, b: string) => void;
}>) {
  const [open, setOpen] = useState<"a" | "b" | null>(null);
  const find = (name: string | null) => channels.find((channel) => channel.name === name);
  const a = find(aName) ?? find("sink_game") ?? channels[0];
  const b = find(bName) ?? find("sink_chat") ?? channels.find((channel) => channel.name !== a?.name);
  if (!a || !b) return null;

  const picker = (side: "a" | "b", selected: VirtualSink, other: VirtualSink) => (
    <div style={{ position: "relative" }}>
      <button type="button" className="select" onClick={() => setOpen(open === side ? null : side)}>
        <span>{selected.label}</span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open === side} onClose={() => setOpen(null)} side="bottom" align="end">
        {channels.filter((channel) => channel.name !== other.name).map((channel) => (
          <MenuItem
            key={channel.name}
            icon={channel.icon ?? "graphic_eq"}
            selected={channel.name === selected.name}
            showCheck
            onClick={() => {
              onPick(
                side === "a" ? channel.name : a.name,
                side === "b" ? channel.name : b.name,
              );
              setOpen(null);
            }}
          >
            {channel.label}
          </MenuItem>
        ))}
      </Popover>
    </div>
  );

  return (
    <div className="row row-sub">
      <div className="ricon"><Ms name="balance" /></div>
      <div className="rmain">
        <div className="rtitle">Wheel channels</div>
        <div className="rsub">The same Balance A/B pair used by the title-bar slider</div>
      </div>
      <div style={{ display: "flex", gap: "var(--sp-2)" }}>
        {picker("a", a, b)}
        {picker("b", b, a)}
      </div>
    </div>
  );
}

function arctisStateLabel(state: ArctisConnectionState): string {
  switch (state) {
    case "connected": return "Connected";
    case "disconnected": return "Disconnected";
    case "permission_denied": return "Permission denied";
    case "unsupported": return "Unsupported";
    default: return "Disabled";
  }
}

function engineDesc(native: boolean | null): string {
  if (native === null) return "…";
  return native
    ? "Native PipeWire (pipewire-rs) - live metering, passive routing"
    : "pactl fallback - native engine unavailable on this system";
}

export function SettingsScreen() {
  const { theme, setTheme } = useTheme();
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [startMinimized, setStartMinimized] = useState(false);
  const [backendNative, setBackendNative] = useState<boolean | null>(null);
  const [version, setVersion] = useState("");
  const [defaults, setDefaults] = useState<DefaultDevices>({ output: null, input: null });
  const [labelStyle, setLabelStyle] = useState<LabelStyle>("plain");
  const [labelStyleOpen, setLabelStyleOpen] = useState(false);
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hardwareChatMix, setHardwareChatMix] = useState(false);
  const [headsetAutoSwitch, setHeadsetAutoSwitch] = useState(false);
  const [arctisOutput, setArctisOutput] = useState<string | null>(null);
  const [arctisStatus, setArctisStatus] = useState<ArctisStatus>({
    state: "disabled",
    detail: null,
  });
  const channels = useMixerStore((s) => s.channels);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const replayOnboarding = useMixerStore((s) => s.replayOnboarding);
  const showBalance = useMixerStore((s) => s.showBalance);
  const setBalanceVisible = useMixerStore((s) => s.setBalanceVisible);
  const balanceA = useMixerStore((s) => s.balanceA);
  const balanceB = useMixerStore((s) => s.balanceB);
  const setBalanceChannels = useMixerStore((s) => s.setBalanceChannels);

  useEffect(() => {
    void invoke<boolean>("get_autostart").then(setAutostart);
    void invoke<{ native: boolean }>("get_backend_info").then((i) => setBackendNative(i.native));
    void invoke<DefaultDevices>("get_default_devices").then(setDefaults).catch(() => {});
    void invoke<{
      device_label_style: LabelStyle;
      start_minimized: boolean;
      hardware_chatmix_enabled: boolean;
      headset_auto_switch: boolean;
      arctis_output: string | null;
    }>("get_prefs")
      .then((p) => {
        setLabelStyle(p.device_label_style);
        setStartMinimized(p.start_minimized);
        setHardwareChatMix(p.hardware_chatmix_enabled);
        setHeadsetAutoSwitch(p.headset_auto_switch);
        setArctisOutput(p.arctis_output);
      })
      .catch(() => {});
    void invoke<ArctisStatus>("get_arctis_status").then(setArctisStatus).catch(() => {});
    const unlisten = listen<ArctisStatus>("arctis-status", (event) => {
      setArctisStatus(event.payload);
    });
    void getVersion().then(setVersion);
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const setHardwarePreference = async (
    command: "set_hardware_chatmix_enabled" | "set_headset_auto_switch",
    enabled: boolean,
  ) => {
    try {
      await invoke(command, { enabled });
      if (command === "set_hardware_chatmix_enabled") setHardwareChatMix(enabled);
      else setHeadsetAutoSwitch(enabled);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const pickArctisOutput = async (output: string | null) => {
    try {
      await invoke("set_arctis_output", { output });
      setArctisOutput(output);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const pickDefault = async (kind: "output" | "input", name: string) => {
    try {
      await invoke(kind === "output" ? "set_default_output" : "set_default_input", { name });
      setDefaults((d) => ({ ...d, [kind]: name }));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const pickLabelStyle = async (style: LabelStyle) => {
    try {
      await invoke("set_device_label_style", { style });
      setLabelStyle(style);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleAutostart = async () => {
    if (autostart === null) return;
    try {
      const actual = await invoke<boolean>("set_autostart", { enabled: !autostart });
      setAutostart(actual);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleStartMinimized = async () => {
    const next = !startMinimized;
    setStartMinimized(next);
    try {
      await invoke("set_start_minimized", { minimized: next });
      setError(null);
    } catch (e) {
      setStartMinimized(!next);
      setError(String(e));
    }
  };

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>Settings</h1>
      </div>
      <div className="screen-scroll">
        {error && <div className="error-banner" style={{ borderRadius: 8 }}>{error}</div>}

        <div className="section-label">Appearance</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="palette" />
            </div>
            <div className="rmain">
              <div className="rtitle">Theme</div>
              <div className="rsub">Original, or Tokyo Night to match your desktop</div>
            </div>
            <div className="theme-picker">
              {THEMES.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className={"theme-swatch" + (t.id === theme ? " active" : "")}
                  onClick={() => setTheme(t.id)}
                  title={t.label}
                >
                  <span className="theme-swatch-colors">
                    {t.swatch.map((c) => (
                      <i key={c} style={{ background: c }} />
                    ))}
                  </span>
                  <span className="theme-swatch-label">{t.label}</span>
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="section-label">Preferences</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="label" />
            </div>
            <div className="rmain">
              <div className="rtitle">Device naming</div>
              <div className="rsub">Naming scheme for Sink-managed devices</div>
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setLabelStyleOpen((o) => !o)}>
                <span>{LABEL_STYLES.find((s) => s.value === labelStyle)?.label}</span>
                <Ms name="expand_more" />
              </button>
              <Popover open={labelStyleOpen} onClose={() => setLabelStyleOpen(false)} side="bottom" align="end">
                {LABEL_STYLES.map((s) => (
                  <MenuItem
                    key={s.value}
                    selected={s.value === labelStyle}
                    showCheck
                    onClick={() => {
                      void pickLabelStyle(s.value);
                      setLabelStyleOpen(false);
                    }}
                  >
                    {s.example}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
          <DeviceRow
            icon="speaker"
            title="Default output"
            sub="Where channels set to “System default” play"
            devices={outputDevices}
            current={defaults.output}
            onPick={(name) => void pickDefault("output", name)}
          />
          <DeviceRow
            icon="mic"
            title="Default input"
            sub="The microphone the Sink mic chain captures"
            devices={inputDevices}
            current={defaults.input}
            onPick={(name) => void pickDefault("input", name)}
          />
          <div className="row">
            <div className="ricon">
              <Ms name="balance" />
            </div>
            <div className="rmain">
              <div className="rtitle">Balance slider</div>
              <div className="rsub">ChatMix-style blend of two channels in the title bar</div>
            </div>
            <Toggle on={showBalance} onClick={() => void setBalanceVisible(!showBalance)} />
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="rocket_launch" />
            </div>
            <div className="rmain">
              <div className="rtitle">Start at login</div>
              <div className="rsub">systemd user service, starts with your desktop session</div>
            </div>
            {autostart !== null && <Toggle on={autostart} onClick={() => void toggleAutostart()} />}
          </div>
          {autostart && (
            <div className="row row-sub">
              <div className="ricon">
                <Ms name="dock_to_bottom" />
              </div>
              <div className="rmain">
                <div className="rtitle">Start minimized</div>
                <div className="rsub">Boot to the tray instead of opening the window</div>
              </div>
              <Toggle
                on={startMinimized}
                onClick={() => void toggleStartMinimized()}
              />
            </div>
          )}
        </div>

        <div className="section-label">SteelSeries Arctis Nova 7</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon"><Ms name="headphones" /></div>
            <div className="rmain">
              <div className="rtitle">Hardware ChatMix</div>
              <div className="rsub">Read the physical wheel through the scoped hidraw interface</div>
            </div>
            <Toggle
              on={hardwareChatMix}
              onClick={() => void setHardwarePreference("set_hardware_chatmix_enabled", !hardwareChatMix)}
            />
          </div>
          <div className="row row-sub">
            <div className="ricon"><Ms name="sensors" /></div>
            <div className="rmain">
              <div className="rtitle">Headset status</div>
              <div className="rsub">{arctisStatus.detail ?? "Hardware monitoring is off"}</div>
            </div>
            <span className={"tag" + (arctisStatus.state === "connected" ? " live" : "")}>
              {arctisStateLabel(arctisStatus.state)}
            </span>
          </div>
          <BalanceChannelsRow
            channels={channels}
            aName={balanceA}
            bName={balanceB}
            onPick={(a, b) => void setBalanceChannels(a, b)}
          />
          <ArctisOutputRow
            devices={outputDevices}
            current={arctisOutput}
            onPick={(output) => void pickArctisOutput(output)}
          />
          <div className="row row-sub">
            <div className="ricon"><Ms name="swap_horiz" /></div>
            <div className="rmain">
              <div className="rtitle">Headset auto-switch</div>
              <div className="rsub">On wireless connect, use Game; on disconnect, restore only if Sink still owns the default</div>
            </div>
            <Toggle
              on={headsetAutoSwitch}
              onClick={() => void setHardwarePreference("set_headset_auto_switch", !headsetAutoSwitch)}
            />
          </div>
        </div>

        <div className="section-label">About</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="cable" />
            </div>
            <div className="rmain">
              <div className="rtitle">Audio engine</div>
              <div className="rsub">
                {engineDesc(backendNative)}
              </div>
            </div>
            {backendNative !== null && (
              <span className={"tag" + (backendNative ? " live" : "")}>
                {backendNative ? "native" : "fallback"}
              </span>
            )}
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="info" />
            </div>
            <div className="rmain">
              <div className="rtitle">Sink {version}</div>
              <div className="rsub">GPL-3.0 · config in ~/.config/sink</div>
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="school" />
            </div>
            <div className="rmain">
              <div className="rtitle">Tutorial</div>
              <div className="rsub">Replay the first-run tour</div>
            </div>
            <button type="button" className="select" onClick={replayOnboarding}>
              <span>Replay</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restart_alt" />
            </div>
            <div className="rmain">
              <div className="rtitle">Reset Sink</div>
              <div className="rsub">
                Erase all channels, mixes, profiles, app history and preferences
              </div>
            </div>
            <button type="button" className="select" onClick={() => setConfirmingReset(true)}>
              <span>Reset…</span>
            </button>
          </div>
        </div>
      </div>

      <ConfirmModal
        open={confirmingReset}
        onClose={() => setConfirmingReset(false)}
        title="Reset Sink?"
        confirmLabel="Reset everything"
        onConfirm={() => void invoke("reset_app").catch((e) => setError(String(e)))}
      >
        Everything you've set up - channels, mixes, profiles, app assignments,
        history and preferences - is permanently deleted, and Sink relaunches
        as if freshly installed.
      </ConfirmModal>
    </div>
  );
}
