import { useEffect, useState } from 'react';

// Hash routes: #/ (everyone), #/home (following), #/@name (profile).
export type View = { kind: 'global' } | { kind: 'home' } | { kind: 'user'; name: string };

function parse(hash: string, loggedIn: boolean): View {
  const m = hash.match(/^#\/@(\w{1,20})$/);
  if (m) return { kind: 'user', name: m[1] };
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
  return v.kind === 'user' ? `user:${v.name.toLowerCase()}` : v.kind;
}

export function go(hash: string) {
  location.hash = hash;
}
