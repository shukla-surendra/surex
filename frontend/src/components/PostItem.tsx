import { useState, type MouseEvent } from 'react';
import { api, type Post, type User } from '../api';
import { go, nameOf } from '../route';
import { useGuard, useToast } from '../toast';
import { Avatar } from './Avatar';
import { MediaGrid } from './MediaGrid';
import { RichText } from './RichText';

function ago(ms: number): string {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/**
 * One post. Clicking the card (outside links and buttons) opens its thread; `focus`
 * renders the post a thread is about, larger and not clickable.
 */
export function PostItem({
  post,
  me,
  fresh,
  focus,
  onDeleted,
}: {
  post: Post;
  me: User | null;
  fresh?: boolean;
  focus?: boolean;
  onDeleted: (id: string) => void;
}) {
  const [liked, setLiked] = useState(post.liked);
  const [likes, setLikes] = useState(post.like_count);
  const [pop, setPop] = useState(false);
  const guard = useGuard();
  const toast = useToast();
  const profile = `#/@${encodeURIComponent(post.author.username)}`;
  const thread = `#/post/${post.id}`;

  const toggleLike = () => {
    if (!me) return toast('Log in to like posts');
    const on = !liked;
    // Optimistic update, rolled back if the request fails.
    setLiked(on);
    setLikes((n) => n + (on ? 1 : -1));
    setPop(on);
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
      onDeleted(post.id);
      toast('Post deleted');
    });

  const open = (e: MouseEvent) => {
    if (focus || (e.target as HTMLElement).closest('a, button') || getSelection()?.toString()) return;
    go(thread);
  };

  return (
    <article className={`card post${fresh ? ' fresh' : ''}${focus ? ' focus' : ' clickable'}`} onClick={open}>
      <a href={profile} aria-label={`@${post.author.username}`}>
        <Avatar username={post.author.username} size={focus ? 52 : 44} />
      </a>
      <div className="post-body">
        <div className="post-meta">
          <a className="name" href={profile}>
            {nameOf(post.author)}
          </a>
          <span className="muted handle">@{post.author.username}</span>
          <a className="muted when" href={thread} title={new Date(post.created_at).toLocaleString()}>
            · {ago(post.created_at)}
          </a>
        </div>
        {post.replying_to && (
          <div className="replying muted">
            Replying to{' '}
            <a className="mention" href={post.reply_to ? `#/post/${post.reply_to}` : `#/@${post.replying_to}`}>
              @{post.replying_to}
            </a>
          </div>
        )}
        {post.body && (
          <div className="post-text">
            <RichText text={post.body} />
          </div>
        )}
        <MediaGrid media={post.media} />
        <div className="post-actions">
          <a className="icon-btn" href={thread} aria-label={`${post.reply_count} replies`}>
            <span aria-hidden="true">↩</span> {post.reply_count}
          </a>
          <button
            className={`icon-btn like${liked ? ' on' : ''}${pop ? ' pop' : ''}`}
            onClick={toggleLike}
            onAnimationEnd={() => setPop(false)}
            aria-pressed={liked}
          >
            <span className="heart">{liked ? '♥' : '♡'}</span> {likes}
          </button>
          {me && post.author.id === me.id && (
            <button className="icon-btn" onClick={remove} style={{ marginLeft: 'auto' }}>
              Delete
            </button>
          )}
        </div>
      </div>
    </article>
  );
}
