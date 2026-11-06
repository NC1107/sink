import { useRef, useState } from "react";

interface NumFieldProps {
  value: number;
  min: number;
  max: number;
  /** Arrow-key nudge. */
  step: number;
  /** How the committed value reads in the field (e.g. whole hertz). */
  format?: (n: number) => string;
  disabled?: boolean;
  title?: string;
  "aria-label": string;
  onChange: (n: number) => void;
}

const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

/** A number box you can actually type into: the text is yours while you
 *  edit, the value follows live whenever it reads as an in-range number, and
 *  clamping waits for Enter or leaving the field. A text input, because a
 *  number input reports "" for half-typed values like "-". */
export function NumField({
  value,
  min,
  max,
  step,
  format = String,
  disabled,
  title,
  "aria-label": ariaLabel,
  onChange,
}: Readonly<NumFieldProps>) {
  const [draft, setDraft] = useState<string | null>(null);
  // The value when editing began, for Escape.
  const start = useRef(value);

  const parse = (text: string) => {
    const n = Number(text.trim());
    return text.trim() !== "" && Number.isFinite(n) ? n : null;
  };

  const edit = (text: string) => {
    if (draft === null) start.current = value;
    setDraft(text);
    const n = parse(text);
    if (n !== null && n >= min && n <= max) onChange(n);
  };

  const commit = () => {
    if (draft === null) return;
    const n = parse(draft);
    // Nothing usable typed: keep what was there.
    if (n !== null) onChange(clamp(n, min, max));
    setDraft(null);
  };

  return (
    <input
      type="text"
      inputMode="decimal"
      value={draft ?? format(value)}
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
      onChange={(e) => edit(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          commit();
        } else if (e.key === "Escape" && draft !== null) {
          // Escape belongs to this field, not the dialog around it.
          e.stopPropagation();
          if (value !== start.current) onChange(start.current);
          setDraft(null);
        } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
          e.preventDefault();
          const base = (draft !== null ? parse(draft) : null) ?? value;
          const next = clamp(base + (e.key === "ArrowUp" ? step : -step), min, max);
          setDraft(null);
          onChange(Number(next.toFixed(6)));
        }
      }}
    />
  );
}
