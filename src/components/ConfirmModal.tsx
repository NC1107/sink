import type { ReactNode } from "react";
import { Modal } from "./Modal";

interface ConfirmModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  /** Label for the destructive action, e.g. "Delete channel". */
  confirmLabel: string;
  onConfirm: () => void;
  /** What the user is about to lose. */
  children: ReactNode;
}

/** Destructive-action confirmation: an explanation and a danger/cancel pair. */
export function ConfirmModal({
  open,
  onClose,
  title,
  confirmLabel,
  onConfirm,
  children,
}: Readonly<ConfirmModalProps>) {
  return (
    <Modal open={open} onClose={onClose} title={title}>
      <p className="modal-text">{children}</p>
      <div className="modal-btns">
        <button
          className="modal-btn danger"
          onClick={() => {
            onClose();
            onConfirm();
          }}
        >
          {confirmLabel}
        </button>
        <button className="modal-btn" onClick={onClose}>
          Cancel
        </button>
      </div>
    </Modal>
  );
}
