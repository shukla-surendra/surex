import { useState } from 'react';
import { api, type User } from '../api';
import { useGuard } from '../toast';

/** Landing card for logged-out visitors: pitch plus a log-in / sign-up form. */
export function AuthForm({ onAuth }: { onAuth: (user: User) => void }) {
  const [mode, setMode] = useState<'login' | 'register'>('login');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const guard = useGuard();

  const submit = () =>
    guard(async () => {
      setBusy(true);
      try {
        const user = await api[mode](username.trim(), password);
        setPassword('');
        onAuth(user);
      } finally {
        setBusy(false);
      }
    });

  return (
    <section className="card hero">
      <h1>
        Say it short.
        <br />
        Say it <em>warm</em>.
      </h1>
      <p className="muted" style={{ margin: 0, maxWidth: 420 }}>
        surex is a small, fast place for 280-character thoughts. Follow people, and your
        home feed fills with what they post.
      </p>

      <div className="auth-tabs" role="tablist">
        <button role="tab" aria-selected={mode === 'login'} className={mode === 'login' ? 'on' : ''} onClick={() => setMode('login')}>
          Log in
        </button>
        <button
          role="tab"
          aria-selected={mode === 'register'}
          className={mode === 'register' ? 'on' : ''}
          onClick={() => setMode('register')}
        >
          Create account
        </button>
      </div>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        style={{ maxWidth: 420 }}
      >
        <input
          className="field"
          placeholder="Username"
          autoComplete="username"
          maxLength={20}
          value={username}
          onChange={(e) => setUsername(e.target.value)}
        />
        <input
          className="field"
          type="password"
          placeholder={mode === 'register' ? 'Password (8+ characters)' : 'Password'}
          autoComplete={mode === 'register' ? 'new-password' : 'current-password'}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <div className="auth-row">
          <span className="muted" style={{ fontSize: 13 }}>
            {mode === 'register' ? 'Letters, digits and _ · 3–20 characters' : 'Welcome back.'}
          </span>
          <button className="btn" type="submit" disabled={busy || !username || !password}>
            {mode === 'register' ? 'Join surex' : 'Log in'}
          </button>
        </div>
      </form>
    </section>
  );
}
