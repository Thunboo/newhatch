import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api } from "./api";
import { messageOf } from "./format";
import { useI18n } from "./i18n";
import type { PayloadSearchLimits, SessionFilters } from "./types";

export type PayloadChip = { id: number; term: string };
const equalTerms = (a: string[] = [], b: string[] = []) =>
  a.length === b.length && a.every((term, index) => term === b[index]);

function readBucketFilters(key: string): Record<string, SessionFilters> {
  try {
    const text = localStorage.getItem(key);
    if (!text || text.length > 20_000) return {};
    const data = JSON.parse(text);
    const result: Record<string, SessionFilters> = {};
    for (const bucket of ["known", "unknown"] as const) {
      const value = data?.[bucket];
      if (!value || typeof value !== "object") continue;
      result[bucket] = { bucket,
        sourceId: Number.isSafeInteger(value.sourceId) && value.sourceId > 0 ? value.sourceId : undefined,
        protocol: ["http", "raw_tcp", "websocket"].includes(value.protocol) ? value.protocol : undefined,
        containsFlag: value.containsFlag === true || undefined,
        payload: Array.isArray(value.payload) && value.payload.length <= 100 ? value.payload.filter((s: unknown): s is string => typeof s === "string" && !!s.trim()) : [],
      };
    }
    return result;
  } catch { return {}; }
}

export function useTrafficFilters(username: string) {
  const { t } = useI18n();
  const storageKey = `newhatch_session_buckets_v1:${encodeURIComponent(username)}`;
  const [saved] = useState(() => readBucketFilters(storageKey));
  const [filters, setFilters] = useState<SessionFilters>(() => ({ ...saved.unknown, bucket: "unknown" }));
  const applied = useRef(filters);
  const [chips, setChips] = useState<PayloadChip[]>(() => (filters.payload ?? []).map((term, id) => ({ id, term })));
  const edited = useRef(chips);
  const nextId = useRef(chips.length);
  const [searchValue, setSearchValue] = useState("");
  const awaitingApplication = useRef(false);
  const [applicationFailed, setApplicationFailed] = useState(false);
  const [limits, setLimits] = useState<PayloadSearchLimits | null>(null);
  const [limitsError, setLimitsError] = useState<string | null>(null);
  const [validationError, setValidationError] = useState<"count" | "length" | null>(null);
  useEffect(() => {
    saved[filters.bucket ?? "unknown"] = { ...filters, cursor: undefined, rulesRevision: undefined };
    try {
      if (Object.values(saved).some((value) => value.sourceId || value.protocol || value.containsFlag || value.payload?.length)) localStorage.setItem(storageKey, JSON.stringify(saved));
      else localStorage.removeItem(storageKey);
    } catch { /* In-memory filters remain usable. */ }
  }, [filters, saved, storageKey]);

  const loadLimits = useCallback(() => {
    setLimitsError(null);
    void api.payloadSearchLimits().then(setLimits).catch((error) => {
      setLimitsError(messageOf(error, t("error.unexpected")));
    });
  }, [t]);
  useEffect(loadLimits, [loadLimits]);

  const isCurrent = useCallback((snapshot: SessionFilters) => applied.current === snapshot, []);
  const onLoaded = useCallback((snapshot: SessionFilters) => {
    if (applied.current !== snapshot) return;
    awaitingApplication.current = false;
    setApplicationFailed(false);
  }, []);
  const onLoadFailed = useCallback((snapshot: SessionFilters) => {
    if (applied.current === snapshot && awaitingApplication.current) setApplicationFailed(true);
  }, []);

  const applyEdited = useCallback((updater?: (current: SessionFilters) => SessionFilters) => {
    const current = applied.current;
    let next = updater ? updater(current) : current;
    let terms = edited.current.map((chip) => chip.term);
    if (next.bucket !== current.bucket) {
      saved[current.bucket ?? "unknown"] = { ...current, cursor: undefined, rulesRevision: undefined };
      next = { ...saved[next.bucket ?? "unknown"], bucket: next.bucket, rulesRevision: current.rulesRevision };
      terms = next.payload ?? [];
      const nextChips = terms.map((term) => ({ id: nextId.current++, term }));
      edited.current = nextChips; setChips(nextChips); setSearchValue(""); setValidationError(null);
    }
    if (next.sourceId === current.sourceId && next.protocol === current.protocol
      && next.containsFlag === current.containsFlag && next.bucket === current.bucket
      && next.rulesRevision === current.rulesRevision && equalTerms(terms, current.payload)) return false;
    const snapshot = { ...next, payload: terms };
    if (!equalTerms(terms, current.payload)) awaitingApplication.current = true;
    setApplicationFailed(false);
    // Update synchronously so repeated blank submissions share one application.
    applied.current = snapshot;
    setFilters(snapshot);
    return true;
  }, [saved]);

  const onSearch = (event: FormEvent) => {
    event.preventDefault();
    const term = searchValue.trim();
    if (!term) { applyEdited(); return; }
    if (!limits) return;
    if (Array.from(term).length > limits.max_term_chars) { setValidationError("length"); return; }
    if (edited.current.length >= limits.max_terms) { setValidationError("count"); return; }
    const next = [...edited.current, { id: nextId.current++, term }];
    edited.current = next;
    setChips(next);
    setSearchValue("");
    setValidationError(null);
    applyEdited();
  };

  const onRemoveChip = (id: number) => {
    const next = edited.current.filter((chip) => chip.id !== id);
    edited.current = next;
    setChips(next);
    setValidationError(null);
  };

  const invalidateRules = useCallback(() => {
    const snapshot = { ...applied.current, rulesRevision: (applied.current.rulesRevision ?? 0) + 1 };
    applied.current = snapshot;
    setFilters(snapshot);
  }, []);

  return {
    filters, applyEdited, invalidateRules, isCurrent, onLoaded, onLoadFailed,
    chipProps: {
      filters, chips, searchValue, limits, limitsError,
      filterError: validationError && limits ? t(validationError === "count" ? "filters.countLimit" : "filters.lengthLimit",
        { count: validationError === "count" ? limits.max_terms : limits.max_term_chars }) : null,
      pendingFilters: !equalTerms(chips.map((chip) => chip.term), filters.payload)
        || applicationFailed,
      onFilters: (updater: (current: SessionFilters) => SessionFilters) => { applyEdited(updater); },
      onSearchValue: (value: string) => { setSearchValue(value); setValidationError(null); },
      onSearch, onRemoveChip, onRetryLimits: loadLimits,
    },
  };
}
