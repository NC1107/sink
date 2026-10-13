import type { ReactNode } from "react";
import { Modal } from "./Modal";

interface ConfirmModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  /** Label for the destructive action, e.g. "Delete channel". */
  confirmLabel: string;
  onConfirm: () => void;
  /** What the user is about to lose, or agree to. */
  children: ReactNode;
  /** "primary" for a non-destructive confirmation such as a download. */
  tone?: "danger" | "primary";
}

/** Confirmation: an explanation and a confirm/cancel pair, destructive by default. */
export function ConfirmModal({
  open,
  onClose,
  title,
  confirmLabel,
  onConfirm,
  children,
  tone = "danger",
}: Readonly<ConfirmModalProps>) {
  return (
    <Modal open={open} onClose={onClose} title={title}>
      <p className="modal-text">{children}</p>
      <div className="modal-btns">
        <button
          type="button"
          className={"modal-btn " + tone}
          onClick={() => {
            onClose();
            onConfirm();
          }}
        >
          {confirmLabel}
        </button>
        <button type="button" className="modal-btn" onClick={onClose}>
          Cancel
        </button>
      </div>
    </Modal>
  );
}
