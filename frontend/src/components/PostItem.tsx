import { useState } from 'react';
import { api, type Post, type User } from '../api';
import { useGuard, useToast } from '../toast';

function ago(ms: number): string {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return new Date(ms).toLocaleDateString();
}

export function PostItem({ post, me, onDeleted }: { post: Post; me: User | null; onDeleted: (id: string) => void }) {
  const [liked, setLiked] = useState(post.liked);
  const [likes, setLikes] = useState(post.like_count);
  const [hidden, setHidden] = useState(false);
  const guard = useGuard();
  const toast = useToast();

  const toggleLike = () => {
    if (!me) return toast('Log in to like posts');
    const on = !liked;
    // Optimistic update, rolled back if the request fails.
    setLiked(on);
    setLikes((n) => n + (on ? 1 : -1));
    guard(async () => {
      try {
        await api.setLike(post.id, on);
      } catch (e) {
        setLiked(!on);
        setLikes((n) => n + (on ? -1 : 1));
        throw e;
      }
    });
  };

  const remove = () =>
    guard(async () => {
      if (!confirm('Delete this post?')) return;
      await api.deletePost(post.id);
      setHidden(true);
      onDeleted(post.id);
    });

  if (hidden) return null;

  return (
    <article className="box">
      <div className="row">
        <a className="u" href={`#/@${encodeURIComponent(post.author.username)}`}>
          @{post.author.username}
        </a>
        <span className="m">{ago(post.created_at)}</span>
      </div>
      <div className="p">{post.body}</div>
      <div className="row">
        <button className={liked ? 'g on' : 'g'} onClick={toggleLike}>
          {liked ? '♥' : '♡'} {likes}
        </button>
        {me && post.author.id === me.id && (
          <button className="g" onClick={remove}>
            delete
          </button>
        )}
      </div>
    </article>
  );
}
