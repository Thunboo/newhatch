import { LogIn, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { api, ApiError, onUnauthorized } from "./api";
import logoUrl from "./assets/logo.png";
import { useI18n } from "./i18n";

type AuthState = "loading" | "authenticated" | "unauthenticated" | "unavailable";

export function AuthGate({ children }: { children: (logout: () => Promise<void>) => ReactNode }) {
  const { t } = useI18n();
  const [state, setState] = useState<AuthState>("loading");
  const [attempt, setAttempt] = useState(0);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => onUnauthorized(() => {
    setState("unauthenticated");
    setPassword("");
    setError("");
  }), []);

  useEffect(() => {
    let current = true;
    setState("loading");
    api.me().then(() => {
      if (current) setState("authenticated");
    }).catch((caught) => {
      if (current) setState(caught instanceof ApiError && caught.status === 401 ? "unauthenticated" : "unavailable");
    });
    return () => { current = false; };
  }, [attempt]);

  async function login(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api.login(username, password);
      setPassword("");
      setState("authenticated");
    } catch (caught) {
      setPassword("");
      setError(caught instanceof ApiError && caught.status === 401
        ? t("auth.invalidCredentials")
        : t("auth.signInUnavailable"));
    } finally { setBusy(false); }
  }

  async function logout() {
    await api.logout();
    setPassword("");
    setError("");
    setState("unauthenticated");
  }

  if (state === "authenticated") return children(logout);

  return (
    <main className="auth-screen">
      <div className="auth-brand"><img src={logoUrl} alt="Нюхач" /></div>
      <section className="auth-content" aria-busy={state === "loading" || busy}>
        {state === "loading" ? <p role="status">{t("auth.checking")}</p> : state === "unavailable" ? <>
          <p role="alert">{t("auth.unavailable")}</p>
          <button className="primary-button" onClick={() => setAttempt((value) => value + 1)}><RefreshCw size={16} /> {t("auth.retry")}</button>
        </> : <form className="auth-form" onSubmit={(event) => void login(event)}>
          <h1>{t("auth.signIn")}</h1>
          <label htmlFor="auth-username">{t("auth.username")}</label>
          <input id="auth-username" name="username" autoComplete="username" autoFocus required maxLength={128} value={username} onChange={(event) => setUsername(event.target.value)} disabled={busy} />
          <label htmlFor="auth-password">{t("auth.password")}</label>
          <input id="auth-password" name="password" type="password" autoComplete="current-password" required maxLength={1024} value={password} onChange={(event) => setPassword(event.target.value)} disabled={busy} />
          {error && <p role="alert" className="auth-error">{error}</p>}
          <button className="primary-button" disabled={busy} type="submit"><LogIn size={16} /> {busy ? t("auth.signingIn") : t("auth.signIn")}</button>
        </form>}
      </section>
    </main>
  );
}
