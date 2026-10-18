import { useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useI18n } from "./i18n";

type Label = { rule_id: number; label: string };

/** One shared, measured layout for table cells and narrow payload headers. */
export function RuleLabels({ matches = [], source }: { matches?: readonly Label[]; source?: string }) {
  const { t } = useI18n();
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const prefix = useRef<HTMLSpanElement>(null);
  const measure = useRef<HTMLSpanElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const restoringFocus = useRef(false);
  const closing = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [firstFits, setFirstFits] = useState(false);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const labelsKey = JSON.stringify(matches.map((match) => [match.rule_id, match.label]));

  useLayoutEffect(() => {
    const button = trigger.current;
    if (!button || !measure.current || !matches.length) return;
    const resize = () => {
      const widths = Array.from(measure.current!.children).map((node) => node.getBoundingClientRect().width);
      const prefixWidth = prefix.current?.getBoundingClientRect().width ?? 0;
      const available = button.getBoundingClientRect().width - prefixWidth - (source ? 6 : 0);
      setFirstFits(available >= widths[0] + (matches.length > 1 ? widths[1] + 4 : 0));
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(button);
    document.fonts.ready.then(() => { if (trigger.current === button) resize(); });
    return () => observer.disconnect();
  }, [labelsKey, source, matches.length]);

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      if (!trigger.current || !popup.current) return;
      const anchor = trigger.current.getBoundingClientRect();
      const box = popup.current.getBoundingClientRect();
      const left = Math.max(8, Math.min(anchor.left, window.innerWidth - box.width - 8));
      const top = anchor.bottom + box.height + 8 <= window.innerHeight
        ? anchor.bottom + 6 : Math.max(8, anchor.top - box.height - 6);
      setPosition({ left, top });
    };
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!trigger.current?.contains(target) && !popup.current?.contains(target)) setOpen(false);
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    document.addEventListener("pointerdown", outside, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
      document.removeEventListener("pointerdown", outside, true);
    };
  }, [open, labelsKey]);
  useLayoutEffect(() => () => clearTimeout(closing.current), []);

  if (!matches.length) return source ? <span className="source-name">{source}</span> : null;
  const show = () => { clearTimeout(closing.current); setOpen(true); };
  const hide = () => { closing.current = setTimeout(() => setOpen(false), 140); };
  const count = matches.length - (firstFits ? 1 : 0);
  const escape = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.stopPropagation(); event.preventDefault(); clearTimeout(closing.current); setOpen(false);
      restoringFocus.current = true; trigger.current?.focus(); restoringFocus.current = false;
    }
  };
  return <span className="rule-labels" onKeyDown={escape}>
    <button ref={trigger} type="button" className="rule-label-trigger" aria-label={`${source ? `${source} · ` : ""}${t("rules.matches", { count: matches.length })}`}
      aria-expanded={open} aria-controls={open ? id : undefined} aria-haspopup="dialog"
      onMouseEnter={show} onMouseLeave={hide} onFocus={() => { if (!restoringFocus.current) show(); }}
      onBlur={(event) => { if (!popup.current?.contains(event.relatedTarget)) setOpen(false); }}
      onClick={(event) => { event.stopPropagation(); show(); }}>
      {source && <span ref={prefix} className="source-name">{source}</span>}
      {firstFits && <span className="known-label">{matches[0].label}</span>}
      {count > 0 && <span className="known-label rule-label-count">+{count}</span>}
    </button>
    <span ref={measure} className="rule-label-measure" aria-hidden="true"><span className="rule-label-metric">{matches[0].label}</span><span className="rule-label-metric">+{Math.max(1, matches.length - 1)}</span></span>
    {open && createPortal(<div ref={popup} id={id} role="dialog" aria-label={t("rules.matchedRules")} tabIndex={0}
      className="rule-label-popup" style={position} onMouseEnter={show} onMouseLeave={hide} onKeyDown={escape}
      onClick={(event) => event.stopPropagation()} onBlur={(event) => {
        if (!trigger.current?.contains(event.relatedTarget) && !popup.current?.contains(event.relatedTarget)) setOpen(false);
      }}>
      {source && <strong>{source}</strong>}
      <span className="rule-popup-heading">{t("rules.matchedRules")}</span>
      <ul>{matches.map((match) => <li key={match.rule_id}><span className="known-label">{match.label}</span></li>)}</ul>
    </div>, document.body)}
  </span>;
}
