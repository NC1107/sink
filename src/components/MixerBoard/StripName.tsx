import { useState } from "react";

interface StripNameProps {
  label: string;
  /** Given a trimmed, changed label. Not called for a no-op edit. */
  onRename: (label: string) => void;
}

/** A strip's label, renamed in place on double-click. */
export function StripName({ label, onRename }: Readonly<StripNameProps>) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const commit = () => {
    setEditing(false);
    const next = draft.trim();
    if (next && next !== label) onRename(next);
  };

  if (editing) {
    return (
      <input
        className="menu-input strip-name-input"
        value={draft}
        autoFocus
        maxLength={24}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setEditing(false);
        }}
      />
    );
  }

  return (
    <div
      className="strip-name strip-name-editable"
      title="Double-click to rename"
      onDoubleClick={() => {
        setDraft(label);
        setEditing(true);
      }}
    >
      {label}
    </div>
  );
}
