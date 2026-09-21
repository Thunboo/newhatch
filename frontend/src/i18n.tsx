import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";

export type Language = "en" | "ru";
type Variables = Record<string, string | number>;

const STORAGE_KEY = "newhatch_language";

const en = {
  "auth.checking": "Checking session...",
  "auth.unavailable": "Unable to connect.",
  "auth.retry": "Retry",
  "auth.signIn": "Sign in",
  "auth.signingIn": "Signing in...",
  "auth.username": "Username",
  "auth.password": "Password",
  "auth.invalidCredentials": "Invalid username or password.",
  "auth.signInUnavailable": "Sign-in unavailable. Please try again.",
  "nav.primary": "Primary navigation",
  "nav.sessions": "Sessions",
  "nav.sources": "Sources",
  "nav.collectors": "Collectors",
  "status.analyzerOnline": "Analyzer online",
  "status.analyzerOffline": "Analyzer offline",
  "status.collectorOnline": "Collector online",
  "status.collectorOffline": "Collector offline",
  "language.target": "Русский",
  "language.action": "Switch to Russian",
  "auth.signOut": "Sign out",
  "auth.signOutFailed": "Sign-out failed. Please try again.",
  "error.unexpected": "Unexpected error",
  "history.noProgress": "History pagination made no progress. Try again.",
  "collectors.localActive": "Local capture active",
  "collectors.connected": "{count} connected",
  "collectors.refresh": "Refresh collectors",
  "collectors.localTitle": "Local capture mode is active",
  "collectors.localDescriptionBefore": "Collector connections are unavailable. Set ",
  "collectors.localDescriptionAfter": " and add sources.",
  "collectors.captured": "Captured",
  "collectors.sent": "Sent",
  "collectors.dropped": "Dropped",
  "collectors.queue": "Queue",
  "collectors.reconnects": "Reconnects",
  "collectors.lastActivity": "Last activity",
  "collectors.empty": "No collectors seen",
  "sessions.all": "All",
  "sessions.rawTcp": "Raw TCP",
  "sessions.loaded": "{count} loaded connections",
  "sessions.refresh": "Refresh sessions",
  "sessions.searchPayload": "Search reconstructed payload",
  "common.clearSearch": "Clear search",
  "sessions.sourceFilter": "Source filter",
  "sessions.allSources": "All sources",
  "sessions.flagsOnly": "Flags only",
  "sessions.protocolFilter": "Protocol filter",
  "sessions.time": "Time",
  "common.source": "Source",
  "sessions.client": "Client",
  "sessions.server": "Server",
  "common.protocol": "Protocol",
  "sessions.size": "Size",
  "sessions.flag": "Flag",
  "sessions.flagMatches": "{count} flag matches",
  "sessions.suricataAlerts": "Suricata alerts",
  "sessions.incomplete": "Incomplete session",
  "sessions.empty": "No sessions match",
  "sessions.loadingOlder": "Loading older sessions...",
  "common.retry": "Retry",
  "sessions.allShown": "All loaded sessions are shown",
  "sources.subtitle": "Kernel capture filter inputs",
  "sources.name": "Name",
  "sources.tcpPort": "TCP port",
  "sources.enabled": "Enabled",
  "sources.disabled": "Disabled",
  "sources.add": "Add source",
  "sources.controls": "Source list controls",
  "sources.search": "Search by name or port",
  "sources.sort": "Sort sources",
  "sources.nameAsc": "Name A-Z",
  "sources.nameDesc": "Name Z-A",
  "sources.portAsc": "Port low-high",
  "sources.portDesc": "Port high-low",
  "sources.port": "Port",
  "sources.status": "Status",
  "sources.delete": "Delete source",
  "sources.deleteConfirm": "Delete source \"{name}\"?",
  "sources.empty": "No monitored sources",
  "sources.noMatch": "No sources match",
  "detail.label": "Session detail",
  "detail.session": "Session #{id}",
  "detail.closeHint": "to close",
  "detail.close": "Close session",
  "detail.started": "Started",
  "detail.traffic": "Traffic",
  "detail.flagMatches": "Flag matches: {count}",
  "detail.formatJson": "Format JSON",
  "detail.payloadFormat": "Payload format",
  "detail.text": "Text",
  "detail.hex": "Hex",
  "detail.clientToServer": "Client -> server",
  "detail.serverToClient": "Server -> client",
  "detail.copied": "Copied",
  "detail.copyFailed": "Copy failed",
  "detail.copy": "Copy {title}",
} as const;

type TranslationKey = keyof typeof en;

const ru: Record<TranslationKey, string> = {
  "auth.checking": "Проверка сессии...",
  "auth.unavailable": "Не удалось подключиться.",
  "auth.retry": "Повторить",
  "auth.signIn": "Войти",
  "auth.signingIn": "Выполняется вход...",
  "auth.username": "Имя пользователя",
  "auth.password": "Пароль",
  "auth.invalidCredentials": "Неверное имя пользователя или пароль.",
  "auth.signInUnavailable": "Вход недоступен. Попробуйте ещё раз.",
  "nav.primary": "Основная навигация",
  "nav.sessions": "Сессии",
  "nav.sources": "Источники",
  "nav.collectors": "Коллекторы",
  "status.analyzerOnline": "Анализатор доступен",
  "status.analyzerOffline": "Анализатор недоступен",
  "status.collectorOnline": "Коллектор доступен",
  "status.collectorOffline": "Коллектор недоступен",
  "language.target": "English",
  "language.action": "Переключить на английский",
  "auth.signOut": "Выйти",
  "auth.signOutFailed": "Не удалось выйти. Попробуйте ещё раз.",
  "error.unexpected": "Непредвиденная ошибка",
  "history.noProgress": "Не удалось продолжить загрузку истории. Попробуйте ещё раз.",
  "collectors.localActive": "Локальный сбор активен",
  "collectors.connected": "Подключено: {count}",
  "collectors.refresh": "Обновить коллекторы",
  "collectors.localTitle": "Запущен режим локального сбора",
  "collectors.localDescriptionBefore": "Подключение коллекторов недоступно. Установите ",
  "collectors.localDescriptionAfter": " и добавьте источники.",
  "collectors.captured": "Захвачено",
  "collectors.sent": "Отправлено",
  "collectors.dropped": "Отброшено",
  "collectors.queue": "Очередь",
  "collectors.reconnects": "Переподключения",
  "collectors.lastActivity": "Последняя активность",
  "collectors.empty": "Коллекторы не обнаружены",
  "sessions.all": "Все",
  "sessions.rawTcp": "Raw TCP",
  "sessions.loaded": "Загружено соединений: {count}",
  "sessions.refresh": "Обновить сессии",
  "sessions.searchPayload": "Поиск по восстановленным данным",
  "common.clearSearch": "Очистить поиск",
  "sessions.sourceFilter": "Фильтр по источнику",
  "sessions.allSources": "Все источники",
  "sessions.flagsOnly": "Только с флагами",
  "sessions.protocolFilter": "Фильтр по протоколу",
  "sessions.time": "Время",
  "common.source": "Источник",
  "sessions.client": "Клиент",
  "sessions.server": "Сервер",
  "common.protocol": "Протокол",
  "sessions.size": "Размер",
  "sessions.flag": "Флаг",
  "sessions.flagMatches": "Совпадений флага: {count}",
  "sessions.suricataAlerts": "Оповещения Suricata",
  "sessions.incomplete": "Неполная сессия",
  "sessions.empty": "Подходящие сессии не найдены",
  "sessions.loadingOlder": "Загрузка более старых сессий...",
  "common.retry": "Повторить",
  "sessions.allShown": "Показаны все загруженные сессии",
  "sources.subtitle": "Входные данные фильтра захвата ядра",
  "sources.name": "Название",
  "sources.tcpPort": "TCP-порт",
  "sources.enabled": "Включён",
  "sources.disabled": "Выключен",
  "sources.add": "Добавить источник",
  "sources.controls": "Управление списком источников",
  "sources.search": "Поиск по названию или порту",
  "sources.sort": "Сортировка источников",
  "sources.nameAsc": "Название А-Я",
  "sources.nameDesc": "Название Я-А",
  "sources.portAsc": "Порт по возрастанию",
  "sources.portDesc": "Порт по убыванию",
  "sources.port": "Порт",
  "sources.status": "Состояние",
  "sources.delete": "Удалить источник",
  "sources.deleteConfirm": "Удалить источник \"{name}\"?",
  "sources.empty": "Нет отслеживаемых источников",
  "sources.noMatch": "Подходящие источники не найдены",
  "detail.label": "Подробности сессии",
  "detail.session": "Сессия #{id}",
  "detail.closeHint": "для закрытия",
  "detail.close": "Закрыть сессию",
  "detail.started": "Начало",
  "detail.traffic": "Трафик",
  "detail.flagMatches": "Совпадений флага: {count}",
  "detail.formatJson": "Форматировать JSON",
  "detail.payloadFormat": "Формат данных",
  "detail.text": "Текст",
  "detail.hex": "Hex",
  "detail.clientToServer": "Клиент -> сервер",
  "detail.serverToClient": "Сервер -> клиент",
  "detail.copied": "Скопировано",
  "detail.copyFailed": "Не удалось скопировать",
  "detail.copy": "Копировать: {title}",
};

const dictionaries: Record<Language, Record<TranslationKey, string>> = { en, ru };

function initialLanguage(): Language {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored === "en" || stored === "ru") return stored;
  } catch {
    // Storage may be disabled; browser language is still a safe default.
  }
  return [navigator.language, ...navigator.languages]
    .some((language) => language.toLowerCase().startsWith("ru")) ? "ru" : "en";
}

type I18nContextValue = {
  language: Language;
  locale: string;
  setLanguage: (language: Language) => void;
  toggleLanguage: () => void;
  t: (key: TranslationKey, variables?: Variables) => string;
};

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<Language>(initialLanguage);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const setLanguage = useCallback((next: Language) => {
    setLanguageState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // The current page still changes language when persistence is unavailable.
    }
  }, []);

  const toggleLanguage = useCallback(() => {
    setLanguage(language === "en" ? "ru" : "en");
  }, [language, setLanguage]);

  const t = useCallback((key: TranslationKey, variables: Variables = {}) => (
    dictionaries[language][key].replace(/\{(\w+)\}/g, (_, name: string) => String(variables[name] ?? `{${name}}`))
  ), [language]);

  const value = useMemo(() => ({
    language,
    locale: language === "ru" ? "ru-RU" : "en-US",
    setLanguage,
    toggleLanguage,
    t,
  }), [language, setLanguage, t, toggleLanguage]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const context = useContext(I18nContext);
  if (!context) throw new Error("useI18n must be used within I18nProvider");
  return context;
}
