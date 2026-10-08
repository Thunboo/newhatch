import { AlertTriangle, Check, Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useI18n } from "./i18n";

export function ExportLogo({ src, fallback, kind }: { src: string; fallback: string; kind: "curl" | "python" }) {
  const [failed, setFailed] = useState(false);
  return failed
    ? <span className="export-fallback" aria-hidden="true">{fallback}</span>
    : <img className={`export-logo export-logo-${kind}`} src={src} alt="" aria-hidden="true" onError={() => setFailed(true)} />;
}

export function PayloadCopyButton({ className, label, disabledReason, loadingLabel, getText, onError, children }: {
  className?: string; label: string; disabledReason?: string; loadingLabel?: string;
  getText: (signal: AbortSignal) => string | Promise<string>; onError?: (error: unknown) => void; children: ReactNode;
}) {
  const { t } = useI18n();
  const [state, setState] = useState<"idle" | "loading" | "copied" | "failed">("idle");
  const resetRef = useRef<number | null>(null);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => {
    request.current?.abort();
    if (resetRef.current !== null) window.clearTimeout(resetRef.current);
  }, []);

  const copy = async () => {
    if (request.current) return;
    const controller = new AbortController();
    request.current = controller;
    if (resetRef.current !== null) window.clearTimeout(resetRef.current);
    setState("loading");
    try {
      const result = getText(controller.signal);
      // Preserve user activation for ordinary synchronous payload exports.
      const text = typeof result === "string" ? result : await result;
      if (controller.signal.aborted) return;
      await copyText(text);
      if (!controller.signal.aborted) setState("copied");
    } catch (error) {
      if (!controller.signal.aborted) { setState("failed"); onError?.(error); }
    } finally {
      if (request.current === controller) request.current = null;
      if (!controller.signal.aborted) resetRef.current = window.setTimeout(() => setState("idle"), 1_500);
    }
  };
  const tooltip = disabledReason ?? (state === "loading" ? loadingLabel ?? t("detail.copyPreparing")
    : state === "copied" ? t("detail.copied") : state === "failed" ? t("detail.copyFailed") : label);
  return <span title={tooltip}>
    <button className={[className, state === "failed" ? "copy-failed" : ""].filter(Boolean).join(" ")} type="button" title={tooltip} aria-label={label} aria-busy={state === "loading"} disabled={!!disabledReason || state === "loading"} onClick={() => void copy()}>
      <span className={`copy-button-content${state === "idle" ? "" : " is-hidden"}`}>{children}</span>
      {state === "loading" && <span className="copy-button-loading"><Loader2 size={14} className="spin" /></span>}
      {state === "copied" && <span className="copy-button-feedback"><Check size={14} /></span>}
      {state === "failed" && <span className="copy-button-feedback"><AlertTriangle size={14} /></span>}
    </button>
  </span>;
}

async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try { await navigator.clipboard.writeText(text); return; }
    catch { /* Fall back on HTTP origins where Clipboard API is restricted. */ }
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  try {
    textarea.select();
    if (!document.execCommand("copy")) throw new Error("Clipboard access was denied");
  } finally { textarea.remove(); }
}
