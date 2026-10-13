import { useState } from "react";
import { useEditTrigger } from "./useEditTrigger";

interface StripNameProps {
  label: string;
  /** Given a trimmed, changed label. Not called for a no-op edit. */
  onRename: (label: string) => void;
}

/** A strip's label, renamed in place on double-click or Enter. */
export function StripName({ label, onRename }: Readonly<StripNameProps>) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const trigger = useEditTrigger(() => {
    setDraft(label);
    setEditing(true);
  });

  const close = () => {
    setEditing(false);
    trigger.restoreFocus();
  };

  const commit = () => {
    close();
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
          if (e.key === "Escape") close();
        }}
      />
    );
  }

  return (
    <div
      {...trigger.props}
      className="strip-name strip-name-editable"
      title="Double-click or press Enter to rename"
      aria-label={`${label}, press Enter to rename`}
    >
      {label}
    </div>
  );
}
