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
export function EqPresetMenu({ sinkName, config, onApply, onError }: EqPresetMenuProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [presets, setPresets] = useState<EqPresetEntry[]>([]);
  const [saveName, setSaveName] = useState("");
  const [importing, setImporting] = useState(false);
  const [importText, setImportText] = useState("");

  const refresh = () => {
    invoke<EqPresetEntry[]>("list_eq_presets")
      .then(setPresets)
      .catch((e) => onError(String(e)));
  };

  useEffect(() => {
    if (menuOpen) refresh();
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
      refresh();
    } catch (e) {
      onError(String(e));
    }
  };

  return (
    <div style={{ position: "relative" }}>
      <button className="select" onClick={() => setMenuOpen((o) => !o)}>
        <Ms name="library_music" style={{ fontSize: 15 }} />
        <span>Presets</span>
        <Ms name="expand_more" />
      </button>
      <Popover
        open={menuOpen}
        onClose={() => {
          setMenuOpen(false);
          setImporting(false);
        }}
        side="bottom"
        align="end"
        style={{ minWidth: 260 }}
      >
        {presets.map((entry) => (
          <div key={`${entry.source}:${entry.preset.name}`} className="eqm-preset-row">
            <button
              className="menu-item eqm-preset-apply"
              title={entry.preset.description ?? undefined}
              onClick={() => applyPreset(entry)}
            >
              <Ms name={entry.source === "bundled" ? "verified" : "person"} />
              <span>{entry.preset.name}</span>
            </button>
            {entry.source === "user" && (
              <button
                className="eqm-remove"
                title="Delete preset"
                aria-label={`Delete preset ${entry.preset.name}`}
                onClick={() => void deletePreset(entry.preset.name)}
              >
                <Ms name="close" style={{ fontSize: 13 }} />
              </button>
            )}
          </div>
        ))}

        <div className="menu-sep" />
        <div className="eqm-save-row">
          <input
            className="menu-input"
            placeholder="Save current as…"
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
            onClick={() => void saveCurrent()}
          >
            <Ms name="save" style={{ fontSize: 15 }} />
          </button>
        </div>

        <div className="menu-sep" />
        {importing ? (
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
                Import
              </button>
              <button className="select" onClick={() => void importFromFile()}>
                From file…
              </button>
            </div>
          </div>
        ) : (
          <button className="menu-item eqm-menu-btn" onClick={() => setImporting(true)}>
            <Ms name="content_paste" />
            <span>Import (paste / AutoEq / file)</span>
          </button>
        )}
        <button className="menu-item eqm-menu-btn" onClick={() => void exportToFile()}>
          <Ms name="download" />
          <span>Export to file…</span>
        </button>
      </Popover>
    </div>
  );
}
