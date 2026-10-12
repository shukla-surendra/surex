import { useCallback, useEffect, useRef, useState } from 'react';
import { api, type Page, type Post, type User } from '../api';
import type { FeedView as View } from '../route';
import { useGuard } from '../toast';
import { PostItem } from './PostItem';

const POLL_MS = 20_000;

function fetchPage(view: View, before: string | null): Promise<Page<Post>> {
  switch (view.kind) {
    case 'home':
      return api.homeTimeline(before);
    case 'user':
      return api.userPosts(view.name, before);
    default:
      return api.globalTimeline(before);
  }
}

/** Snowflake ids are 64-bit decimal strings; compare them exactly. */
const newer = (a: string, b: string) => BigInt(a) > BigInt(b);

function Skeleton() {
  return (
    <div className="card skeleton" aria-hidden="true">
      <div className="shimmer" style={{ width: 44, height: 44, borderRadius: 14 }} />
      <div style={{ flex: 1 }}>
        <div className="shimmer" style={{ width: '30%', height: 12, marginBottom: 10 }} />
        <div className="shimmer" style={{ width: '90%', height: 12, marginBottom: 8 }} />
        <div className="shimmer" style={{ width: '60%', height: 12 }} />
      </div>
    </div>
  );
}

const EMPTY: Record<View['kind'], [string, string]> = {
  home: ['Your feed is quiet', 'Follow people from the Everyone tab and their posts land here.'],
  global: ['Nothing posted yet', 'Be the first to say something.'],
  user: ['No posts yet', 'When they post, it will show up here.'],
};

/**
 * Remounted (via `key`) whenever the view or the logged-in user changes. Loads more
 * when the end comes into view, and polls for newer posts to offer a "new posts" pill.
 */
export function Feed({ view, me, fresh }: { view: View; me: User | null; fresh: Post[] }) {
  const [posts, setPosts] = useState<Post[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [incoming, setIncoming] = useState(0);
  const [removed, setRemoved] = useState<Set<string>>(() => new Set());
  const sentinel = useRef<HTMLDivElement>(null);
  const guard = useGuard();

  const load = useCallback(
    (before: string | null) =>
      guard(async () => {
        setLoading(true);
        try {
          const page = await fetchPage(view, before);
          setPosts((prev) => (before ? [...prev, ...page.items] : page.items));
          setNext(page.next);
          setLoaded(true);
          if (!before) setIncoming(0);
        } finally {
          setLoading(false);
        }
      }),
    // `view` is fixed for this component's lifetime (the parent remounts it via `key`).
    [guard],
  );

  useEffect(() => {
    load(null);
  }, [load]);

  // Infinite scroll: fetch the next page as the sentinel nears the viewport.
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !next || loading) return;
    const io = new IntersectionObserver((entries) => entries[0].isIntersecting && load(next), {
      rootMargin: '600px',
    });
    io.observe(el);
    return () => io.disconnect();
  }, [next, loading, load]);

  // Poll for newer posts while the tab is visible.
  const top = fresh[0]?.id ?? posts[0]?.id;
  useEffect(() => {
    if (!loaded) return;
    const tick = async () => {
      if (document.hidden) return;
      try {
        const page = await fetchPage(view, null);
        setIncoming(top ? page.items.filter((p) => newer(p.id, top)).length : page.items.length);
      } catch {
        // Polling is best-effort.
      }
    };
    const id = window.setInterval(tick, POLL_MS);
    return () => window.clearInterval(id);
    // `view` is fixed for this component's lifetime.
  }, [loaded, top]);

  const showIncoming = () => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
    load(null);
  };

  const freshIds = new Set(fresh.map((p) => p.id));
  const items = [...fresh, ...posts.filter((p) => !freshIds.has(p.id))].filter((p) => !removed.has(p.id));
  const remove = (id: string) => setRemoved((prev) => new Set(prev).add(id));

  if (!loaded) {
    return (
      <>
        <Skeleton />
        <Skeleton />
        <Skeleton />
      </>
    );
  }

  const [title, hint] = EMPTY[view.kind];
  return (
    <>
      {incoming > 0 && (
        <div className="pill">
          <button onClick={showIncoming}>
            ↑ {incoming >= 30 ? '30+' : incoming} new {incoming === 1 ? 'post' : 'posts'}
          </button>
        </div>
      )}
      {items.map((p) => (
        <PostItem key={p.id} post={p} me={me} fresh={freshIds.has(p.id)} onDeleted={remove} />
      ))}
      {items.length === 0 && !next && (
        <div className="card empty">
          <div className="big">{title}</div>
          <div className="muted">{hint}</div>
        </div>
      )}
      {next && (
        <div ref={sentinel}>
          <Skeleton />
        </div>
      )}
      {!next && items.length > 0 && (
        <p className="muted" style={{ textAlign: 'center', fontSize: 13 }}>
          You're all caught up.
        </p>
      )}
    </>
  );
}
