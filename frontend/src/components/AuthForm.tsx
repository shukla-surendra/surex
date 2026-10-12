import { useState } from 'react';
import { api, type User } from '../api';
import { useGuard } from '../toast';

export function AuthForm({ onAuth }: { onAuth: (user: User) => void }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const guard = useGuard();

  const submit = (mode: 'login' | 'register') =>
    guard(async () => {
      const user = await api[mode](username.trim(), password);
      setPassword('');
      onAuth(user);
    });

  return (
    <form
      className="box"
      onSubmit={(e) => {
        e.preventDefault();
        submit('login');
      }}
    >
      <input
        placeholder="username"
        autoComplete="username"
        maxLength={20}
        value={username}
        onChange={(e) => setUsername(e.target.value)}
      />
      <input
        type="password"
        placeholder="password (8+ characters)"
        autoComplete="current-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <div className="row">
        <span className="m">Log in or create an account</span>
        <span>
          <button type="submit">Log in</button>{' '}
          <button type="button" className="o" onClick={() => submit('register')}>
            Sign up
          </button>
        </span>
      </div>
    </form>
  );
}
