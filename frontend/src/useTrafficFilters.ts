import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api } from "./api";
import { messageOf } from "./format";
import { useI18n } from "./i18n";
import type { PayloadSearchLimits, SessionFilters } from "./types";

export type PayloadChip = { id: number; term: string };
const equalTerms = (a: string[] = [], b: string[] = []) =>
  a.length === b.length && a.every((term, index) => term === b[index]);

export function useTrafficFilters() {
  const { t } = useI18n();
  const [filters, setFilters] = useState<SessionFilters>({});
  const applied = useRef(filters);
  const [chips, setChips] = useState<PayloadChip[]>([]);
  const edited = useRef(chips);
  const nextId = useRef(0);
  const [searchValue, setSearchValue] = useState("");
  const awaitingApplication = useRef(false);
  const [applicationFailed, setApplicationFailed] = useState(false);
  const [limits, setLimits] = useState<PayloadSearchLimits | null>(null);
  const [limitsError, setLimitsError] = useState<string | null>(null);
  const [validationError, setValidationError] = useState<"count" | "length" | null>(null);

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
    const next = updater ? updater(current) : current;
    const terms = edited.current.map((chip) => chip.term);
    if (next.sourceId === current.sourceId && next.protocol === current.protocol
      && next.containsFlag === current.containsFlag && equalTerms(terms, current.payload)) return false;
    const snapshot = { ...next, payload: terms };
    if (!equalTerms(terms, current.payload)) awaitingApplication.current = true;
    setApplicationFailed(false);
    // Update synchronously so repeated blank submissions share one application.
    applied.current = snapshot;
    setFilters(snapshot);
    return true;
  }, []);

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

  return {
    filters, applyEdited, isCurrent, onLoaded, onLoadFailed,
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
