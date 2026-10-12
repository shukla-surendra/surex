import { useEffect, useState } from 'react';

// Hash routes:
//   #/ (everyone)  #/home (following)  #/@name (profile)
//   #/@name/followers  #/@name/following  #/post/<id> (thread)
export type FeedView = { kind: 'global' } | { kind: 'home' } | { kind: 'user'; name: string };
export type View =
  | FeedView
  | { kind: 'post'; id: string }
  | { kind: 'followers' | 'following'; name: string };

function parse(hash: string, loggedIn: boolean): View {
  let m = hash.match(/^#\/@(\w{1,20})\/(followers|following)$/);
  if (m) return { kind: m[2] as 'followers' | 'following', name: m[1] };
  m = hash.match(/^#\/@(\w{1,20})$/);
  if (m) return { kind: 'user', name: m[1] };
  m = hash.match(/^#\/post\/(\d{1,20})$/);
  if (m) return { kind: 'post', id: m[1] };
  if (hash === '#/home' && loggedIn) return { kind: 'home' };
  return { kind: 'global' };
}

export function useHash(): string {
  const [hash, setHash] = useState(location.hash);
  useEffect(() => {
    const onChange = () => setHash(location.hash);
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return hash;
}

export function useView(loggedIn: boolean): View {
  return parse(useHash(), loggedIn);
}

export function viewKey(v: View): string {
  switch (v.kind) {
    case 'post':
      return `post:${v.id}`;
    case 'user':
    case 'followers':
    case 'following':
      return `${v.kind}:${v.name.toLowerCase()}`;
    default:
      return v.kind;
  }
}

export function isFeed(v: View): v is FeedView {
  return v.kind === 'global' || v.kind === 'home' || v.kind === 'user';
}

export function go(hash: string) {
  location.hash = hash;
}

/** Display name, falling back to the handle. */
export function nameOf(u: { username: string; display_name: string }): string {
  return u.display_name || u.username;
}
