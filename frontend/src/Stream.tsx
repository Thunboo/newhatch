import { Copy, Flag, Search } from "lucide-react";
import { useMemo } from "react";
import type { ReactNode } from "react";
import curlLogoUrl from "./assets/Curl-logo.svg";
import pythonLogoUrl from "./assets/python-logo.png";
import { useI18n } from "./i18n";
import { payloadHighlights } from "./payloadHighlights";
import type { HighlightRange } from "./payloadHighlights";
import { exportCurl, exportPython, parseReplayRequest, replayProblemKeys } from "./requestExport";
import { ExportLogo, PayloadCopyButton } from "./PayloadCopyButton";
import type { ReplayTarget } from "./requestExport";
import type { ByteRange, WindowRule } from "./types";
import { RuleLabels } from "./RuleLabels";
import { formatBytes } from "./format";
export type PayloadMode = "text" | "hex";
const NO_PAYLOAD_TERMS: readonly string[] = [];
const NO_RULES: readonly WindowRule[] = [];

export function Stream({ title, subtitle, direction, bytes, ranges, mode, formatJson, flagged, replayTarget, chainScrollLane = false, payloadTerms = NO_PAYLOAD_TERMS, rules = NO_RULES }: { title: string; subtitle?: string; direction: "c2s" | "s2c"; bytes: Uint8Array; ranges: ByteRange[]; mode: PayloadMode; formatJson: boolean; flagged: boolean; replayTarget?: ReplayTarget; chainScrollLane?: boolean; payloadTerms?: readonly string[]; rules?: readonly WindowRule[] }) {
  const { t } = useI18n();
  const displayed = useMemo(() => {
    return payloadHighlights(bytes, payloadTerms, ranges, formatJson, rules.flatMap((rule) => formatJson ? rule.pretty_ranges : rule.ranges));
  }, [bytes, formatJson, payloadTerms, ranges, rules]);
  const displayedText = displayed.text;
  const text = useMemo(() => mode === "hex" ? toHex(bytes) : displayedText, [bytes, displayedText, mode]);
  const replay = useMemo(
    () => replayTarget ? parseReplayRequest(bytes, displayedText, replayTarget) : null,
    [bytes, displayedText, replayTarget?.host, replayTarget?.port],
  );
  const exportDisabled = mode === "hex" ? t("detail.exportHex")
    : replay && !replay.ok ? t(replayProblemKeys[replay.problem]) : undefined;
  const uniqueFlags = useMemo(() => direction === "s2c" ? countUniqueFlags(bytes, ranges) : 0, [bytes, ranges, direction]);

  return (
    <section className={["stream", flagged || uniqueFlags > 0 ? "flagged" : "", uniqueFlags > 0 ? "response-flagged" : ""].filter(Boolean).join(" ")}>
      <header>
        <span className="stream-title">
          <span className="stream-title-line">
            <span className="stream-title-text">{title}</span>
            {uniqueFlags > 0 && <ResponseFlagBadge unique={uniqueFlags} occurrences={ranges.length} />}
            {displayed.hasPayloadMatch && <span className="stream-payload-match" role="img" aria-label={t("detail.payloadMatch")} title={t("detail.payloadMatch")} tabIndex={0}><Search size={13} aria-hidden="true" /></span>}
            {rules.length > 0 && <RuleLabels matches={rules} />}
          </span>
          {subtitle && <small>{subtitle}</small>}
        </span>
        <span className="stream-actions">
          <span className="mono">{formatBytes(bytes.length)}</span>
          {replayTarget && <>
            <PayloadCopyButton className="export-action export-action-curl" label={t("detail.copyBash")} disabledReason={exportDisabled} getText={() => replay?.ok ? exportCurl(replay.request) : ""}>
              <ExportLogo src={curlLogoUrl} fallback="cUrl" kind="curl" />
            </PayloadCopyButton>
            <PayloadCopyButton className="export-action export-action-python" label={t("detail.copyPython")} disabledReason={exportDisabled} getText={() => replay?.ok ? exportPython(replay.request) : ""}>
              <ExportLogo src={pythonLogoUrl} fallback="python" kind="python" />
            </PayloadCopyButton>
          </>}
          <PayloadCopyButton label={t("detail.copy", { title })} getText={() => text}><Copy size={14} /></PayloadCopyButton>
        </span>
      </header>
      <pre>{mode === "text" && displayed.ranges.length > 0 ? <HighlightedText text={text} positions={displayed.ranges} /> : text}</pre>
      {/* A sibling of pre routes native wheel/touch scrolling to the chain body. */}
      {chainScrollLane && <div className="chain-scroll-lane" aria-hidden="true" />}
    </section>
  );
}

function countUniqueFlags(bytes: Uint8Array, ranges: ByteRange[]): number {
  const flags = new Set<string>();
  for (const { start, end, value } of ranges) {
    if (value !== undefined) { flags.add(value); continue; }
    // One code unit per captured byte preserves identity even for invalid UTF-8.
    // Chunk conversion avoids argument-size limits for a long regex match.
    const chunks: string[] = [];
    for (let offset = start; offset < end; offset += 8_192) {
      chunks.push(String.fromCharCode(...bytes.subarray(offset, Math.min(offset + 8_192, end))));
    }
    flags.add(chunks.join(""));
  }
  return flags.size;
}

function ResponseFlagBadge({ unique, occurrences }: { unique: number; occurrences: number }) {
  const { locale, t } = useI18n();
  const plurals = useMemo(() => new Intl.PluralRules(locale), [locale]);
  const uniqueForm = plurals.select(unique);
  const occurrenceForm = plurals.select(occurrences);
  const summary = t("detail.flagSummary", {
    unique: t(uniqueForm === "one" ? "detail.uniqueFlagsOne" : uniqueForm === "few" ? "detail.uniqueFlagsFew" : "detail.uniqueFlagsOther", { count: unique.toLocaleString(locale) }),
    occurrences: t(occurrenceForm === "one" ? "detail.flagOccurrencesOne" : occurrenceForm === "few" ? "detail.flagOccurrencesFew" : "detail.flagOccurrencesOther", { count: occurrences.toLocaleString(locale) }),
  });
  return <span className="stream-flag-count" role="img" aria-label={summary} title={summary} tabIndex={0}><Flag size={13} aria-hidden="true" /><span>{unique.toLocaleString(locale)}</span></span>;
}

function HighlightedText({ text, positions }: { text: string; positions: HighlightRange[] }) {
  const chunks: ReactNode[] = [];
  let offset = 0;
  for (const range of positions) {
    if (range.start < offset) continue;
    if (range.start > offset) chunks.push(text.slice(offset, range.start));
    const className = range.kind === "both" ? "flag-match payload-match" : range.kind === "flag" ? "flag-match" : "payload-match";
    chunks.push(<mark className={className} key={`${range.start}-${range.end}`}>{text.slice(range.start, range.end)}</mark>);
    offset = range.end;
  }
  if (offset < text.length) chunks.push(text.slice(offset));
  return <>{chunks}</>;
}

function toHex(bytes: Uint8Array): string {
  const lines: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const slice = bytes.slice(offset, offset + 16);
    const hex = Array.from(slice, (byte) => byte.toString(16).padStart(2, "0")).join(" ").padEnd(47, " ");
    const ascii = Array.from(slice, (byte) => byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : ".").join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  ${ascii}`);
  }
  return lines.join("\n");
}
