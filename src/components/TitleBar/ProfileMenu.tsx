import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { IconButton } from "../IconButton";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";

/**
 * Profile picker. Profiles are live-bound: every mixer change autosaves
 * into the active profile, so rows just switch - there is no Save button.
 */
export function ProfileMenu() {
  const [open, setOpen] = useState(false);
  const [newName, setNewName] = useState("");
  /** Profile whose auto-switch (trigger) panel is expanded. */
  const [triggerFor, setTriggerFor] = useState<string | null>(null);
  const profiles = useMixerStore((s) => s.profiles);
  const activeProfile = useMixerStore((s) => s.activeProfile);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const loadProfile = useMixerStore((s) => s.loadProfile);
  const deleteProfile = useMixerStore((s) => s.deleteProfile);
  const setProfileTrigger = useMixerStore((s) => s.setProfileTrigger);
  const createBlankProfile = useMixerStore((s) => s.createBlankProfile);

  const close = () => {
    setOpen(false);
    setTriggerFor(null);
    setNewName("");
  };

  const create = () => {
    const name = newName.trim();
    if (!name) return;
    void createBlankProfile(name);
    close();
  };

  const triggerLabel = (device: string | null) => {
    if (!device) return null;
    return outputDevices.find((d) => d.name === device)?.description ?? device;
  };

  return (
    <div style={{ position: "relative" }}>
      <button type="button" className="select" onClick={() => setOpen((o) => !o)} title="Profiles">
        <Ms name="bookmarks" />
        <span>{activeProfile ?? "Profiles"}</span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open} onClose={close} side="bottom" align="end" style={{ minWidth: 240 }}>
        {profiles.map((profile) => {
          const isActive = profile.name === activeProfile;
          const trigger = triggerLabel(profile.trigger_device);
          return (
            <div key={profile.name}>
              {/* The actions are siblings of the load button, never children:
                  a button inside a button is invalid, and the old nesting
                  needed stopPropagation on every action to stay usable. */}
              <div className="profile-row">
                <MenuItem
                  className="profile-row-btn"
                  icon={isActive ? "check" : "bookmark"}
                  selected={isActive}
                  onClick={() => {
                    if (!isActive) void loadProfile(profile.name);
                    close();
                  }}
                >
                  <span className="profile-row-main">
                    <span>{profile.name}</span>
                    {trigger && (
                      <span className="profile-row-trigger">
                        <Ms name="bolt" style={{ fontSize: 12 }} />
                        <span className="profile-row-trigger-name">
                          auto-loads with {trigger}
                        </span>
                      </span>
                    )}
                  </span>
                </MenuItem>
                <div className="profile-row-actions">
                  <IconButton
                    boxed
                    size={15}
                    icon="bolt"
                    title="Auto-load when a device connects"
                    label={`Auto-switch settings for ${profile.name}`}
                    onClick={() => setTriggerFor((t) => (t === profile.name ? null : profile.name))}
                  />
                  <IconButton
                    boxed
                    danger
                    size={15}
                    icon="delete"
                    title="Delete profile"
                    label={`Delete profile ${profile.name}`}
                    onClick={() => void deleteProfile(profile.name)}
                  />
                </div>
              </div>
              {triggerFor === profile.name && (
                <div className="trigger-panel">
                  <div className="trigger-hint">Auto-load when this device connects:</div>
                  <MenuItem
                    icon="block"
                    selected={profile.trigger_device === null}
                    onClick={() => {
                      void setProfileTrigger(profile.name, null);
                      setTriggerFor(null);
                    }}
                  >
                    No auto-switch
                  </MenuItem>
                  {outputDevices.map((d) => (
                    <MenuItem
                      key={d.name}
                      icon="speaker"
                      selected={d.name === profile.trigger_device}
                      onClick={() => {
                        void setProfileTrigger(profile.name, d.name);
                        setTriggerFor(null);
                      }}
                    >
                      {d.description}
                    </MenuItem>
                  ))}
                </div>
              )}
            </div>
          );
        })}

        <div className="menu-sep" />
        <div className="menu-save">
          <input
            className="menu-input"
            placeholder="New profile name…"
            value={newName}
            maxLength={64}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") create();
            }}
          />
          <button
            type="button"
            className="select"
            onClick={create}
            disabled={!newName.trim()}
            title="New blank profile"
          >
            <Ms name="add" />
            <span>Create</span>
          </button>
        </div>
      </Popover>
    </div>
  );
}
