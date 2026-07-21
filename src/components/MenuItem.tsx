import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { Ms } from "./Icons";

interface MenuItemProps {
  /** Leading Material symbol. Omitted for text-only rows. */
  icon?: string;
  /** The current choice: accent styling plus, optionally, a trailing check. */
  selected?: boolean;
  /** Show the trailing check mark. Menus that signal choice by colour alone
   *  leave this off. */
  showCheck?: boolean;
  onClick: () => void;
  title?: string;
  className?: string;
  children: ReactNode;
}

/**
 * One choice in a Popover menu. A real button element, so every menu is
 * reachable by keyboard and announced as a control rather than as text.
 */
export function MenuItem({
  icon,
  selected = false,
  showCheck = false,
  onClick,
  title,
  className,
  children,
}: Readonly<MenuItemProps>) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      className={cx("menu-item", selected && "sel", className)}
      title={title}
      onClick={onClick}
    >
      {icon && <Ms name={icon} />}
      <span className="menu-item-label">{children}</span>
      {showCheck && selected && <Ms name="check" className="menu-item-check" />}
    </button>
  );
}

interface MenuCheckItemProps {
  checked: boolean;
  onClick: () => void;
  title?: string;
  className?: string;
  children: ReactNode;
}

/** A menu row that toggles rather than picks: membership lists and flags. */
export function MenuCheckItem({
  checked,
  onClick,
  title,
  className,
  children,
}: Readonly<MenuCheckItemProps>) {
  return (
    <button
      type="button"
      role="menuitemcheckbox"
      aria-checked={checked}
      className={cx("menu-item", className)}
      title={title}
      onClick={onClick}
    >
      <Ms
        name={checked ? "check_box" : "check_box_outline_blank"}
        className={cx(checked && "menu-check-on")}
      />
      {children}
    </button>
  );
}
