import { useState } from 'react';
import { api, type Post } from '../api';
import { useGuard } from '../toast';

const MAX_CHARS = 280;

export function Composer({ onPosted }: { onPosted: (post: Post) => void }) {
  const [body, setBody] = useState('');
  const [sending, setSending] = useState(false);
  const guard = useGuard();

  const send = () =>
    guard(async () => {
      setSending(true);
      try {
        onPosted(await api.createPost(body));
        setBody('');
      } finally {
        setSending(false);
      }
    });

  return (
    <section className="box">
      <textarea
        rows={3}
        maxLength={MAX_CHARS}
        placeholder="What's happening?"
        value={body}
        onChange={(e) => setBody(e.target.value)}
      />
      <div className="row">
        <span className="m">{MAX_CHARS - [...body].length}</span>
        <button disabled={sending} onClick={send}>
          Post
        </button>
      </div>
    </section>
  );
}
