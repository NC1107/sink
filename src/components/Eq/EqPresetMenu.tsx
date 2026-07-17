import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import type { EqConfig } from "../../types";
import { Ms } from "../Icons";
import { Popover } from "../Popover";

interface EqPresetEntry {
  source: "bundled" | "user";
  preset: {
    schema: number;
    name: string;
    author?: string | null;
    description?: string | null;
    preamp_db: number;
    bands: EqConfig["bands"];
  };
}

interface EqPresetMenuProps {
  sinkName: string;
  config: EqConfig;
  onApply: (config: EqConfig) => void;
  onError: (message: string) => void;
}

/** Preset picker + import/export. Bundled presets ship inside the binary
 *  (repo presets/eq/); user presets live in ~/.config/sink/eq_presets. */
export function EqPresetMenu({ sinkName, config, onApply, onError }: Readonly<EqPresetMenuProps>) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [presets, setPresets] = useState<EqPresetEntry[]>([]);
  const [saveName, setSaveName] = useState("");
  const [importing, setImporting] = useState(false);
  const [importText, setImportText] = useState("");
  // Name of the user preset awaiting a delete confirmation, if any.
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const refresh = () => {
    invoke<EqPresetEntry[]>("list_eq_presets")
      .then(setPresets)
      .catch((e) => onError(String(e)));
  };

  // Fetch on mount (so the button can name the active preset right away) and
  // again whenever the menu opens (to pick up newly saved user presets).
  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menuOpen]);

  const applyPreset = (entry: EqPresetEntry) => {
    setMenuOpen(false);
    // A preset carries bands + preamp; applying always switches the EQ on.
    onApply({
      enabled: true,
      preamp_db: entry.preset.preamp_db,
      bands: entry.preset.bands.map((b) => ({ ...b })),
    });
  };

  const saveCurrent = async () => {
    const name = saveName.trim();
    if (!name) return;
    try {
      await invoke("save_user_eq_preset", { name, config });
      setSaveName("");
      refresh();
    } catch (e) {
      onError(String(e));
    }
  };

  const applyImported = (imported: EqConfig) => {
    // Imports parse to a disabled config (preview semantics); keep the
    // channel's current on/off state so importing never surprises.
    onApply({ ...imported, enabled: config.enabled });
    setImporting(false);
    setImportText("");
    setMenuOpen(false);
  };

  const importPasted = async () => {
    try {
      applyImported(await invoke<EqConfig>("import_eq_config", { text: importText }));
    } catch (e) {
      onError(String(e));
    }
  };

  const importFromFile = async () => {
    try {
      const path = await openDialog({
        multiple: false,
        filters: [{ name: "EQ preset", extensions: ["json", "txt"] }],
      });
      if (typeof path !== "string") return;
      applyImported(await invoke<EqConfig>("import_eq_file", { path }));
    } catch (e) {
      onError(String(e));
    }
  };

  const exportToFile = async () => {
    try {
      const path = await saveDialog({
        defaultPath: `${sinkName.replace(/^sink_/, "")}-eq.json`,
        filters: [{ name: "EQ preset", extensions: ["json"] }],
      });
      if (typeof path !== "string") return;
      await invoke("export_channel_eq_to_file", { sinkName, path });
      setMenuOpen(false);
    } catch (e) {
      onError(String(e));
    }
  };

  const deletePreset = async (name: string) => {
    try {
      await invoke("delete_user_eq_preset", { name });
      setConfirmDelete(null);
      refresh();
    } catch (e) {
      onError(String(e));
    }
  };

  const bundled = presets.filter((p) => p.source === "bundled");
  const user = presets.filter((p) => p.source === "user");

  // The button names whichever preset the current curve matches exactly; any
  // manual edit breaks the match and it falls back to the generic label.
  // Both sides come through the same f32 pipeline, so equality is safe.
  const sameBands = (a: EqConfig["bands"], b: EqConfig["bands"]) =>
    a.length === b.length &&
    a.every(
      (x, i) =>
        x.kind === b[i].kind &&
        x.freq_hz === b[i].freq_hz &&
        x.gain_db === b[i].gain_db &&
        x.q === b[i].q,
    );
  const activePreset = presets.find(
    (e) => e.preset.preamp_db === config.preamp_db && sameBands(config.bands, e.preset.bands),
  );

  const presetRow = (entry: EqPresetEntry) => (
    <div key={`${entry.source}:${entry.preset.name}`} className="eqm-preset-row">
      <button
        className={"menu-item eqm-preset-apply" + (entry === activePreset ? " sel" : "")}
        title={entry.preset.description ?? undefined}
        onClick={() => applyPreset(entry)}
      >
        <Ms name="graphic_eq" />
        <span className="eqm-preset-name">{entry.preset.name}</span>
      </button>
      {entry.source === "user" &&
        (confirmDelete === entry.preset.name ? (
          <span className="eqm-preset-confirm">
            <button
              className="eqm-remove danger"
              title="Delete this preset"
              aria-label={`Confirm delete ${entry.preset.name}`}
              onClick={() => void deletePreset(entry.preset.name)}
            >
              <Ms name="check" style={{ fontSize: 14 }} />
            </button>
            <button
              className="eqm-remove"
              title="Keep it"
              aria-label="Cancel delete"
              onClick={() => setConfirmDelete(null)}
            >
              <Ms name="close" style={{ fontSize: 13 }} />
            </button>
          </span>
        ) : (
          <button
            className="eqm-remove"
            title="Delete preset"
            aria-label={`Delete preset ${entry.preset.name}`}
            onClick={() => setConfirmDelete(entry.preset.name)}
          >
            <Ms name="close" style={{ fontSize: 13 }} />
          </button>
        ))}
    </div>
  );

  return (
    <div style={{ position: "relative" }}>
      <button
        className="select"
        onClick={() => setMenuOpen((o) => !o)}
        title={activePreset ? `Preset: ${activePreset.preset.name}` : undefined}
      >
        <Ms name="library_music" style={{ fontSize: 15 }} />
        <span className="eqm-preset-btn-label">
          {activePreset ? activePreset.preset.name : "Custom"}
        </span>
        <Ms name="expand_more" />
      </button>
      <Popover
        open={menuOpen}
        onClose={() => {
          setMenuOpen(false);
          setImporting(false);
          setConfirmDelete(null);
        }}
        side="bottom"
        align="end"
        style={{ minWidth: 260 }}
      >
        {bundled.length > 0 && (
          <>
            <div className="eqm-preset-head">Bundled</div>
            {bundled.map(presetRow)}
          </>
        )}
        {user.length > 0 && (
          <>
            <div className="eqm-preset-head">Your presets</div>
            {user.map(presetRow)}
          </>
        )}

        <div className="menu-sep" />
        <div className={"eqm-save-label" + (activePreset ? "" : " custom")}>
          {activePreset ? "Save a copy as a new preset" : "Custom curve - save it to keep"}
        </div>
        <div className="eqm-save-row">
          <input
            className="menu-input"
            placeholder="Preset name…"
            value={saveName}
            maxLength={64}
            onChange={(e) => setSaveName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void saveCurrent();
            }}
          />
          <button
            className="select"
            disabled={!saveName.trim()}
            title="Save current curve as a preset"
            onClick={() => void saveCurrent()}
          >
            <Ms name="save" style={{ fontSize: 15 }} />
          </button>
        </div>

        <div className="menu-sep" />
        <div className="eqm-io-row">
          <button
            className={"select eqm-io-btn" + (importing ? " on" : "")}
            aria-expanded={importing}
            title="Import a preset (paste JSON / AutoEq, or a file)"
            onClick={() => setImporting((v) => !v)}
          >
            <Ms name="content_paste" style={{ fontSize: 15 }} />
            <span>Import</span>
          </button>
          <button
            className="select eqm-io-btn"
            title="Export this curve to a JSON file"
            onClick={() => void exportToFile()}
          >
            <Ms name="download" style={{ fontSize: 15 }} />
            <span>Export</span>
          </button>
        </div>
        {importing && (
          <div className="eqm-import">
            <textarea
              className="eqm-import-text"
              placeholder={"Paste preset JSON or an AutoEq block:\nPreamp: -6.0 dB\nFilter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70"}
              value={importText}
              autoFocus
              onChange={(e) => setImportText(e.target.value)}
            />
            <div className="eqm-import-btns">
              <button
                className="select"
                disabled={!importText.trim()}
                onClick={() => void importPasted()}
              >
                Apply pasted
              </button>
              <button className="select" onClick={() => void importFromFile()}>
                From file…
              </button>
            </div>
          </div>
        )}
      </Popover>
    </div>
  );
}
