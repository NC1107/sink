import { cx } from "../lib/cx";
import { Ms } from "./Icons";

interface IconButtonProps {
  icon: string;
  /** Tooltip text, and the accessible name unless `label` overrides it. */
  title: string;
  /** Accessible name for when the tooltip is too terse to name its target
   *  (e.g. "Ignore" on a list of apps). */
  label?: string;
  /** Hidden until the containing `.row` is hovered - list-row actions. */
  reveal?: boolean;
  /** Boxed 24px hit area with a hover fill, for always-visible actions. */
  boxed?: boolean;
  danger?: boolean;
  /** Glyph size in px; varies by row density. */
  size?: number;
  onClick: () => void;
}

/** Borderless icon-only action button used in list rows and menus. */
export function IconButton({
  icon,
  title,
  label,
  reveal,
  boxed,
  danger,
  size = 16,
  onClick,
}: Readonly<IconButtonProps>) {
  return (
    <button
      type="button"
      className={cx("icon-btn", reveal && "reveal", boxed && "boxed", danger && "danger")}
      title={title}
      aria-label={label ?? title}
      onClick={onClick}
    >
      <Ms name={icon} style={{ fontSize: size }} />
    </button>
  );
}
