import { LogIn, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { api, ApiError, onUnauthorized } from "./api";

type AuthState = "loading" | "authenticated" | "unauthenticated" | "unavailable";

export function AuthGate({ children }: { children: (logout: () => Promise<void>) => ReactNode }) {
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
        ? "Invalid username or password."
        : "Sign-in unavailable. Please try again.");
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
      <div className="auth-brand"><img src="/logo.png" alt="Нюхач" /></div>
      <section className="auth-content" aria-busy={state === "loading" || busy}>
        {state === "loading" ? <p role="status">Checking session...</p> : state === "unavailable" ? <>
          <p role="alert">Unable to connect.</p>
          <button className="primary-button" onClick={() => setAttempt((value) => value + 1)}><RefreshCw size={16} /> Retry</button>
        </> : <form className="auth-form" onSubmit={(event) => void login(event)}>
          <h1>Sign in</h1>
          <label htmlFor="auth-username">Username</label>
          <input id="auth-username" name="username" autoComplete="username" autoFocus required maxLength={128} value={username} onChange={(event) => setUsername(event.target.value)} disabled={busy} />
          <label htmlFor="auth-password">Password</label>
          <input id="auth-password" name="password" type="password" autoComplete="current-password" required maxLength={1024} value={password} onChange={(event) => setPassword(event.target.value)} disabled={busy} />
          {error && <p role="alert" className="auth-error">{error}</p>}
          <button className="primary-button" disabled={busy} type="submit"><LogIn size={16} /> {busy ? "Signing in..." : "Sign in"}</button>
        </form>}
      </section>
    </main>
  );
}
