import { useEffect, useState } from 'react';
import { api, type Post, type User } from './api';
import { go, useView, viewKey } from './route';
import { ToastProvider, useGuard } from './toast';
import { AuthForm } from './components/AuthForm';
import { Composer } from './components/Composer';
import { Feed } from './components/Feed';
import { ProfileCard } from './components/ProfileCard';

export function App() {
  return (
    <ToastProvider>
      <Shell />
    </ToastProvider>
  );
}

function Shell() {
  // `undefined` while the session check is in flight, `null` when logged out.
  const [me, setMe] = useState<User | null | undefined>(undefined);
  const [fresh, setFresh] = useState<Post[]>([]);
  const view = useView(!!me);
  const guard = useGuard();

  useEffect(() => {
    api.me().then(
      (user) => {
        setMe(user);
        if (!location.hash) go('#/home');
      },
      () => setMe(null),
    );
  }, []);

  // Posts written in this session are shown on top until the next navigation.
  const key = viewKey(view);
  useEffect(() => setFresh([]), [key]);

  const onAuth = (user: User) => {
    setMe(user);
    go('#/home');
  };
  const logout = () =>
    guard(async () => {
      await api.logout();
      setMe(null);
      go('#/');
    });
  const onPosted = (post: Post) => {
    if (view.kind !== 'user' || view.name.toLowerCase() === post.author.username.toLowerCase()) {
      setFresh((prev) => [post, ...prev]);
    }
  };

  if (me === undefined) return <main />;

  return (
    <main>
      <header>
        <a href="#/" className="brand">
          surex
        </a>
        {me && (
          <>
            <a href={`#/@${me.username}`} className="m">
              @{me.username}
            </a>
            <button className="o" onClick={logout}>
              Log out
            </button>
          </>
        )}
      </header>

      {me ? <Composer onPosted={onPosted} /> : <AuthForm onAuth={onAuth} />}

      {view.kind === 'user' ? (
        <ProfileCard key={key} name={view.name} me={me} />
      ) : (
        <nav>
          <a href="#/home" className={view.kind === 'home' ? 'on' : ''}>
            Following
          </a>
          <a href="#/" className={view.kind === 'global' ? 'on' : ''}>
            Everyone
          </a>
        </nav>
      )}

      <Feed key={`${key}:${me?.id ?? ''}`} view={view} me={me} fresh={fresh} />
    </main>
  );
}
