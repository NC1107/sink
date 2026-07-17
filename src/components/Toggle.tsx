import { Ms } from "./Icons";

/** Design-system switch. */
export function Toggle({ on, onClick }: Readonly<{ on: boolean; onClick: () => void }>) {
  return <button className={"toggle" + (on ? " on" : "")} onClick={onClick} aria-pressed={on} />;
}

/** Card row with an icon, title/subtitle and a trailing switch. */
export function ToggleRow({
  icon,
  title,
  sub,
  on,
  onToggle,
}: Readonly<{
  icon: string;
  title: string;
  sub: string;
  on: boolean;
  onToggle: () => void;
}>) {
  return (
    <div className="row">
      <div className="ricon">
        <Ms name={icon} />
      </div>
      <div className="rmain">
        <div className="rtitle">{title}</div>
        <div className="rsub">{sub}</div>
      </div>
      <Toggle on={on} onClick={onToggle} />
    </div>
  );
}
