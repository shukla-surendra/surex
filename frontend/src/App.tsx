import { useEffect, useState } from 'react';
import { api, type Post, type User } from './api';
import { go, isFeed, nameOf, useView, viewKey, type View } from './route';
import { useTheme, type Theme } from './theme';
import { ToastProvider, useGuard } from './toast';
import { AuthForm } from './components/AuthForm';
import { Avatar } from './components/Avatar';
import { Composer } from './components/Composer';
import { Feed } from './components/Feed';
import { ProfileCard } from './components/ProfileCard';
import { Thread } from './components/Thread';
import { UserList } from './components/UserList';

export function App() {
  return (
    <ToastProvider>
      <Shell />
    </ToastProvider>
  );
}

const THEME_LABEL: Record<Theme, string> = { system: '◐ Auto theme', dark: '☾ Dark theme', light: '☀ Light theme' };

function Brand() {
  return (
    <a href="#/" className="brand">
      <span className="brand-mark">s</span>
      surex
    </a>
  );
}

function Shell() {
  // `undefined` while the session check is in flight, `null` when logged out.
  const [me, setMe] = useState<User | null | undefined>(undefined);
  const [fresh, setFresh] = useState<Post[]>([]);
  const [theme, cycleTheme] = useTheme();
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

  // `g h` / `g e` jump between feeds.
  useEffect(() => {
    let armed = false;
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest('input, textarea') || e.metaKey || e.ctrlKey) return;
      if (armed && e.key === 'h') go('#/home');
      if (armed && e.key === 'e') go('#/');
      armed = e.key === 'g';
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

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

  if (me === undefined) return null;

  const isMine = view.kind === 'user' && me && view.name.toLowerCase() === me.username.toLowerCase();

  return (
    <div className="shell">
      <Rail view={view} me={me} isMine={!!isMine} theme={theme} cycleTheme={cycleTheme} logout={logout} />

      <main className="column">
        <div className="topbar">
          <Brand />
          <button className="icon-btn" onClick={cycleTheme} aria-label="Change theme">
            {THEME_LABEL[theme].slice(0, 1)}
          </button>
        </div>

        {!me && view.kind === 'global' && <AuthForm onAuth={onAuth} />}
        {me && (view.kind === 'home' || view.kind === 'global') && <Composer me={me} onPosted={onPosted} />}

        {view.kind === 'post' && <Thread key={key} id={view.id} me={me} />}
        {(view.kind === 'followers' || view.kind === 'following') && (
          <UserList key={key} name={view.name} kind={view.kind} />
        )}

        {view.kind === 'user' && <ProfileCard key={key} name={view.name} me={me} onMeChanged={setMe} />}
        {(view.kind === 'home' || view.kind === 'global') && (
          <div className="feed-head">
            <div>
              <div className="section-title">{view.kind === 'home' ? 'Following' : 'Everyone'}</div>
              <div className="muted">
                {view.kind === 'home' ? 'Posts from people you follow' : 'Every public post, newest first'}
              </div>
            </div>
            {me && (
              <div className="segmented">
                <a href="#/home" className={view.kind === 'home' ? 'on' : ''}>
                  Following
                </a>
                <a href="#/" className={view.kind === 'global' ? 'on' : ''}>
                  Everyone
                </a>
              </div>
            )}
          </div>
        )}

        {isFeed(view) && <Feed key={`${key}:${me?.id ?? ''}`} view={view} me={me} fresh={fresh} />}
      </main>

      <Aside me={me} />

      <nav className="tabbar">
        {me && (
          <a href="#/home" className={view.kind === 'home' ? 'on' : ''}>
            Following
          </a>
        )}
        <a href="#/" className={view.kind === 'global' ? 'on' : ''}>
          Everyone
        </a>
        {me && (
          <a href={`#/@${me.username}`} className={isMine ? 'on' : ''}>
            Profile
          </a>
        )}
        {me ? <button onClick={logout}>Log out</button> : view.kind !== 'global' && <a href="#/">Log in</a>}
      </nav>
    </div>
  );
}

function Rail({
  view,
  me,
  isMine,
  theme,
  cycleTheme,
  logout,
}: {
  view: View;
  me: User | null;
  isMine: boolean;
  theme: Theme;
  cycleTheme: () => void;
  logout: () => void;
}) {
  return (
    <aside className="rail">
      <Brand />
      {me && (
        <a href="#/home" className={`rail-link${view.kind === 'home' ? ' on' : ''}`}>
          <span className="ico">⌂</span> Following
        </a>
      )}
      <a href="#/" className={`rail-link${view.kind === 'global' ? ' on' : ''}`}>
        <span className="ico">◎</span> Everyone
      </a>
      {me && (
        <a href={`#/@${me.username}`} className={`rail-link${isMine ? ' on' : ''}`}>
          <span className="ico">◉</span> Profile
        </a>
      )}

      {!me && view.kind !== 'global' && (
        <a href="#/" className="btn" style={{ margin: '12px 12px 0', textAlign: 'center' }}>
          Log in or join
        </a>
      )}

      <div className="rail-foot">
        <button className="rail-link" onClick={cycleTheme}>
          {THEME_LABEL[theme]}
        </button>
        {me && (
          <div className="me-chip">
            <Avatar username={me.username} size={34} />
            <span className="who">
              {nameOf(me)}
              <span className="muted handle-sm">@{me.username}</span>
            </span>
            <button className="icon-btn" onClick={logout} style={{ fontSize: 13 }}>
              Log out
            </button>
          </div>
        )}
      </div>
    </aside>
  );
}

function Aside({ me }: { me: User | null }) {
  return (
    <aside className="aside">
      <div className="card">
        <h3>Shortcuts</h3>
        {me && (
          <div className="shortcut">
            New post <span className="kbd">n</span>
          </div>
        )}
        {me && (
          <div className="shortcut">
            Send post
            <span>
              <span className="kbd">⌘</span> <span className="kbd">Enter</span>
            </span>
          </div>
        )}
        {me && (
          <div className="shortcut">
            Following feed
            <span>
              <span className="kbd">g</span> <span className="kbd">h</span>
            </span>
          </div>
        )}
        <div className="shortcut">
          Everyone feed
          <span>
            <span className="kbd">g</span> <span className="kbd">e</span>
          </span>
        </div>
      </div>
      <div className="card">
        <h3>How surex works</h3>
        <p className="muted" style={{ margin: 0, fontSize: 14 }}>
          Posts are up to 280 characters. Mention someone with <b>@name</b> to link their
          profile. New posts from your feed appear as a pill at the top, so your place is
          never lost.
        </p>
      </div>
    </aside>
  );
}
