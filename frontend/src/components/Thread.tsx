import { useEffect, useState } from 'react';
import { api, type Post, type User } from '../api';
import { useGuard } from '../toast';
import { Composer } from './Composer';
import { PostItem } from './PostItem';

/** A post, a link to what it replies to, a reply box, and its replies (oldest first). */
export function Thread({ id, me }: { id: string; me: User | null }) {
  const [post, setPost] = useState<Post | null>(null);
  const [missing, setMissing] = useState(false);
  const [replies, setReplies] = useState<Post[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const guard = useGuard();

  const loadReplies = (after: string | null) =>
    guard(async () => {
      setLoading(true);
      try {
        const page = await api.replies(id, after);
        setReplies((prev) => (after ? [...prev, ...page.items] : page.items));
        setNext(page.next);
      } finally {
        setLoading(false);
      }
    });

  useEffect(() => {
    api.getPost(id).then(
      (p) => {
        setPost(p);
        loadReplies(null);
      },
      () => setMissing(true),
    );
    // The parent remounts this component per thread id.
  }, []);

  if (missing) {
    return (
      <div className="card empty">
        <div className="big">This post isn't available</div>
        <div className="muted">It may have been deleted.</div>
      </div>
    );
  }
  if (!post) return <div className="card skeleton shimmer" style={{ height: 160 }} />;

  const onReplied = (reply: Post) => {
    setReplies((prev) => [...prev, reply]);
    setPost({ ...post, reply_count: post.reply_count + 1 });
  };
  const onDeleted = (rid: string) => {
    setReplies((prev) => prev.filter((r) => r.id !== rid));
    setPost({ ...post, reply_count: Math.max(0, post.reply_count - 1) });
  };

  return (
    <>
      <a className="back-link" href={post.reply_to ? `#/post/${post.reply_to}` : '#/'}>
        ← {post.reply_to ? 'Show the post this replies to' : 'Back to everyone'}
      </a>
      <PostItem key={post.id} post={post} me={me} focus onDeleted={() => history.back()} />
      {me ? (
        <Composer me={me} replyTo={post} onPosted={onReplied} />
      ) : (
        <p className="muted" style={{ textAlign: 'center' }}>
          <a className="mention" href="#/">
            Log in
          </a>{' '}
          to reply.
        </p>
      )}
      <div className="thread-label muted">
        {post.reply_count === 0 ? 'No replies yet' : `${post.reply_count} ${post.reply_count === 1 ? 'reply' : 'replies'}`}
      </div>
      <div className="replies">
        {replies.map((r) => (
          <PostItem key={r.id} post={r} me={me} onDeleted={onDeleted} />
        ))}
      </div>
      {next && (
        <div style={{ textAlign: 'center' }}>
          <button className="btn ghost" disabled={loading} onClick={() => loadReplies(next)}>
            Show more replies
          </button>
        </div>
      )}
    </>
  );
}
