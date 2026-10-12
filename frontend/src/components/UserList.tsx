import { useEffect, useState } from 'react';
import { api, type User } from '../api';
import { nameOf } from '../route';
import { useGuard } from '../toast';
import { Avatar } from './Avatar';
import { RichText } from './RichText';

/** Followers or followees of `name`, paged forward. */
export function UserList({ name, kind }: { name: string; kind: 'followers' | 'following' }) {
  const [users, setUsers] = useState<User[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const guard = useGuard();

  const load = (after: string | null) =>
    guard(async () => {
      const page = await api[kind](name, after);
      setUsers((prev) => (after ? [...prev, ...page.items] : page.items));
      setNext(page.next);
      setLoaded(true);
    });

  useEffect(() => {
    load(null);
    // The parent remounts this component per (name, kind).
  }, []);

  const profile = `#/@${encodeURIComponent(name)}`;
  return (
    <>
      <a className="back-link" href={profile}>
        ← @{name}
      </a>
      <div className="feed-head">
        <div>
          <div className="section-title">{kind === 'followers' ? 'Followers' : 'Following'}</div>
          <div className="muted">
            {kind === 'followers' ? `People who follow @${name}` : `People @${name} follows`}
          </div>
        </div>
        <div className="segmented">
          <a href={`${profile}/followers`} className={kind === 'followers' ? 'on' : ''}>
            Followers
          </a>
          <a href={`${profile}/following`} className={kind === 'following' ? 'on' : ''}>
            Following
          </a>
        </div>
      </div>

      {loaded && users.length === 0 && (
        <div className="card empty">
          <div className="big">{kind === 'followers' ? 'No followers yet' : 'Not following anyone yet'}</div>
        </div>
      )}
      <div className="card people">
        {users.map((u) => (
          <a key={u.id} className="person" href={`#/@${encodeURIComponent(u.username)}`}>
            <Avatar username={u.username} size={42} />
            <span style={{ minWidth: 0 }}>
              <span className="name">{nameOf(u)}</span> <span className="muted">@{u.username}</span>
              {u.bio && (
                <span className="person-bio">
                  <RichText text={u.bio} />
                </span>
              )}
            </span>
          </a>
        ))}
      </div>
      {next && (
        <div style={{ textAlign: 'center', marginTop: 12 }}>
          <button className="btn ghost" onClick={() => load(next)}>
            Show more
          </button>
        </div>
      )}
    </>
  );
}
