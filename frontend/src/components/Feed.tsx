import { useCallback, useEffect, useState } from 'react';
import { api, type Page, type Post, type User } from '../api';
import type { View } from '../route';
import { useGuard } from '../toast';
import { PostItem } from './PostItem';

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

/** Remounted (via `key`) whenever the view or the logged-in user changes. */
export function Feed({ view, me, fresh }: { view: View; me: User | null; fresh: Post[] }) {
  const [posts, setPosts] = useState<Post[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const guard = useGuard();

  const load = useCallback(
    (before: string | null) =>
      guard(async () => {
        const page = await fetchPage(view, before);
        setPosts((prev) => (before ? [...prev, ...page.items] : page.items));
        setNext(page.next);
        setLoaded(true);
      }),
    // `view` is fixed for this component's lifetime (the parent remounts it via `key`).
    [guard],
  );

  useEffect(() => {
    load(null);
  }, [load]);

  const seen = new Set(fresh.map((p) => p.id));
  const items = [...fresh, ...posts.filter((p) => !seen.has(p.id))];
  const remove = (id: string) => setPosts((prev) => prev.filter((p) => p.id !== id));

  return (
    <>
      {items.map((p) => (
        <PostItem key={p.id} post={p} me={me} onDeleted={remove} />
      ))}
      {next && (
        <div className="box c">
          <button className="g" onClick={() => load(next)}>
            Load more
          </button>
        </div>
      )}
      {loaded && items.length === 0 && !next && <div className="box c m">Nothing here yet.</div>}
    </>
  );
}
