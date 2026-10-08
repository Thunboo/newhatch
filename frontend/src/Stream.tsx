import { Copy } from "lucide-react";
import { useMemo } from "react";
import type { ReactNode } from "react";
import curlLogoUrl from "./assets/Curl-logo.svg";
import pythonLogoUrl from "./assets/python-logo.png";
import { useI18n } from "./i18n";
import { decodeDisplayEscapes, decodePayload, displayPayload } from "./payloadDisplay";
import { exportCurl, exportPython, parseReplayRequest, replayProblemKeys } from "./requestExport";
import { ExportLogo, PayloadCopyButton } from "./PayloadCopyButton";
import type { ReplayTarget } from "./requestExport";
import type { ByteRange } from "./types";
import { formatBytes } from "./format";
export type PayloadMode = "text" | "hex";

export function Stream({ title, subtitle, bytes, ranges, mode, formatJson, flagged, replayTarget, chainScrollLane = false }: { title: string; subtitle?: string; bytes: Uint8Array; ranges: ByteRange[]; mode: PayloadMode; formatJson: boolean; flagged: boolean; replayTarget?: ReplayTarget; chainScrollLane?: boolean }) {
  const { t } = useI18n();
  const displayedText = useMemo(() => displayPayload(bytes, formatJson), [bytes, formatJson]);
  const text = useMemo(() => mode === "hex" ? toHex(bytes) : displayedText, [bytes, displayedText, mode]);
  const replay = useMemo(
    () => replayTarget ? parseReplayRequest(bytes, displayedText, replayTarget) : null,
    [bytes, displayedText, replayTarget?.host, replayTarget?.port],
  );
  const exportDisabled = mode === "hex" ? t("detail.exportHex")
    : replay && !replay.ok ? t(replayProblemKeys[replay.problem]) : undefined;
  const matchTexts = useMemo(
    () => ranges.map((range) => decodeDisplayEscapes(decodePayload(bytes.slice(range.start, range.end)))).filter(Boolean),
    [bytes, ranges],
  );

  return (
    <section className={flagged ? "stream flagged" : "stream"}>
      <header>
        <span className="stream-title">{title}{subtitle && <small>{subtitle}</small>}</span>
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
      <pre>{mode === "text" && matchTexts.length > 0 ? <HighlightedText text={text} matches={matchTexts} /> : text}</pre>
      {/* A sibling of pre routes native wheel/touch scrolling to the chain body. */}
      {chainScrollLane && <div className="chain-scroll-lane" aria-hidden="true" />}
    </section>
  );
}

function HighlightedText({ text, matches }: { text: string; matches: string[] }) {
  const positions: ByteRange[] = [];
  const cursors = new Map<string, number>();
  for (const match of matches) {
    const start = text.indexOf(match, cursors.get(match) ?? 0);
    if (start < 0) continue;
    positions.push({ start, end: start + match.length });
    cursors.set(match, start + match.length);
  }
  positions.sort((left, right) => left.start - right.start);

  const chunks: ReactNode[] = [];
  let offset = 0;
  for (const range of positions) {
    if (range.start < offset) continue;
    if (range.start > offset) chunks.push(text.slice(offset, range.start));
    chunks.push(<mark key={`${range.start}-${range.end}`}>{text.slice(range.start, range.end)}</mark>);
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
