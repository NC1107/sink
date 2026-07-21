import { useEffect } from "react";
import type { ReactNode } from "react";
import { IconButton } from "./IconButton";

/** Centered modal dialog with a dimming scrim. Escape or scrim-click closes. */
export function Modal({
  open,
  onClose,
  title,
  children,
  className,
}: Readonly<{
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  /** Extra class on the dialog (e.g. a width variant). */
  className?: string;
}>) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div className="modal-scrim" onClick={onClose}>
      <div
        className={"modal" + (className ? ` ${className}` : "")}
        role="dialog"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <div className="modal-title">{title}</div>
          <IconButton boxed icon="close" title="Close" onClick={onClose} size={18} />
        </div>
        {children}
      </div>
    </div>
  );
}
