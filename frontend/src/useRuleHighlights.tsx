import { useEffect, useState } from "react";
import { api, ApiError } from "./api";
import { useI18n } from "./i18n";
import type { RuleHighlights, RuleMatch } from "./types";

const EMPTY: RuleHighlights = { representation: "readable-v1", c2s: [], s2c: [] };

export function useRuleHighlights(id: number, matches: readonly RuleMatch[] = [], enabled = true, cache?: Map<string, RuleHighlights>) {
  const { t } = useI18n();
  // Retain the classifier's step index and transaction ordinal. Never select
  // another occurrence by searching a loaded context member locally.
  const selection = JSON.stringify(matches.filter((match) => match.version).flatMap((match) => {
    const steps = match.events.flatMap((event, step) => event.session_id === id ? [[step, event.transaction]] : []);
    return steps.length ? [[match.rule_id, match.version, steps]] : [];
  }));
  const key = `${id}:${selection}`;
  const [retry, setRetry] = useState(0);
  const [state, setState] = useState<{ key: string; data: RuleHighlights; error: "changed" | "failed" | null } | undefined>(() => {
    const data = cache?.get(key);
    return data ? { key, data, error: null } : undefined;
  });
  useEffect(() => {
    if (!enabled || selection === "[]") return;
    const cached = cache?.get(key);
    if (cached) { setState({ key, data: cached, error: null }); return; }
    const controller = new AbortController();
    api.getRuleHighlights(id, selection, controller.signal)
      .then((data) => { if (!controller.signal.aborted) { cache?.set(key, data); setState({ key, data, error: null }); } })
      .catch((error) => { if (!controller.signal.aborted) setState({ key, data: EMPTY, error: error instanceof ApiError && error.status === 409 ? "changed" : "failed" }); });
    return () => controller.abort();
  }, [id, selection, key, enabled, retry, cache]);
  const current = state?.key === key ? state : undefined;
  const pending = enabled && selection !== "[]" && !current;
  const error = current?.error ?? null;
  return {
    data: current?.data ?? EMPTY,
    status: pending || error ? <div className="rule-evidence-status" role="status">
      {t(error === "changed" ? "rules.highlightsChanged" : error ? "rules.highlightError" : "rules.highlightLoading")}
      {error && <button type="button" onClick={() => { setState(undefined); setRetry((value) => value + 1); }}>{t("common.retry")}</button>}
    </div> : null,
  };
}
