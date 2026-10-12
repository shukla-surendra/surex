// Generated avatars: a username always maps to the same earthy tone and initial, so
// people are recognisable without uploaded images.

const TONES = ['#c2532d', '#3d6b4a', '#8a5a2b', '#5b5f97', '#a23b5a', '#2f6f73', '#7a6a2f', '#6b4a8a'];

export function tone(username: string): string {
  let h = 0;
  for (const ch of username.toLowerCase()) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return TONES[h % TONES.length];
}

export function Avatar({ username, size = 44 }: { username: string; size?: number }) {
  const base = tone(username);
  return (
    <span
      className="avatar"
      aria-hidden="true"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.42,
        background: `linear-gradient(140deg, ${base}, color-mix(in srgb, ${base} 60%, #1d1a16))`,
      }}
    >
      {username.slice(0, 1)}
    </span>
  );
}
