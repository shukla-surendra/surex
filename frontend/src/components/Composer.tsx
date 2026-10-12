import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent } from 'react';
import { api, type Media, type Post, type User } from '../api';
import { useGuard, useToast } from '../toast';
import { Avatar } from './Avatar';

const MAX_CHARS = 280;
const MAX_IMAGES = 4;
const MAX_BYTES = 5 * 1024 * 1024;
const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];

/** An image being attached: a local preview until the upload returns. */
interface Draft {
  key: string;
  preview: string;
  uploaded?: Media;
  alt: string;
}

/** Remaining-characters ring: fills as you type, turns warning-coloured near the limit. */
function Ring({ used }: { used: number }) {
  const r = 10;
  const c = 2 * Math.PI * r;
  const left = MAX_CHARS - used;
  const stroke = left < 0 ? 'var(--danger)' : left <= 20 ? 'var(--accent)' : 'var(--moss)';
  return (
    <span className="ring" aria-label={`${left} characters left`}>
      <svg width="26" height="26" viewBox="0 0 26 26" aria-hidden="true">
        <circle cx="13" cy="13" r={r} fill="none" stroke="var(--line)" strokeWidth="3" />
        <circle
          cx="13"
          cy="13"
          r={r}
          fill="none"
          stroke={stroke}
          strokeWidth="3"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={c * (1 - Math.min(used / MAX_CHARS, 1))}
          transform="rotate(-90 13 13)"
        />
      </svg>
      {left <= 40 && <span style={{ color: stroke }}>{left}</span>}
    </span>
  );
}

export function Composer({
  me,
  replyTo,
  onPosted,
}: {
  me: User;
  /** When set, the composer writes a reply to this post. */
  replyTo?: Post;
  onPosted: (post: Post) => void;
}) {
  const [body, setBody] = useState('');
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [sending, setSending] = useState(false);
  const [dragging, setDragging] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const guard = useGuard();
  const toast = useToast();
  const used = [...body].length;
  const uploading = drafts.some((d) => !d.uploaded);
  const canSend = !sending && !uploading && used <= MAX_CHARS && (body.trim() !== '' || drafts.length > 0);

  // `n` focuses the composer from anywhere (unless already typing somewhere).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (e.key === 'n' && !e.metaKey && !e.ctrlKey && !e.altKey && !t.closest('input, textarea')) {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const addFiles = (files: File[]) => {
    const images = files.filter((f) => IMAGE_TYPES.includes(f.type));
    if (images.length < files.length) toast('Only PNG, JPEG, GIF and WebP images can be attached', 'error');
    const room = MAX_IMAGES - drafts.length;
    if (images.length > room) toast(`A post can have at most ${MAX_IMAGES} images`, 'error');
    for (const file of images.slice(0, Math.max(room, 0))) {
      if (file.size > MAX_BYTES) {
        toast(`${file.name} is larger than 5 MB`, 'error');
        continue;
      }
      const key = `${file.name}:${file.size}:${Math.random()}`;
      setDrafts((prev) => [...prev, { key, preview: URL.createObjectURL(file), alt: '' }]);
      api.uploadMedia(file).then(
        (uploaded) => setDrafts((prev) => prev.map((d) => (d.key === key ? { ...d, uploaded } : d))),
        (e: Error) => {
          setDrafts((prev) => prev.filter((d) => d.key !== key));
          toast(e.message, 'error');
        },
      );
    }
  };

  const removeDraft = (key: string) =>
    setDrafts((prev) => {
      const gone = prev.find((d) => d.key === key);
      if (gone) URL.revokeObjectURL(gone.preview);
      return prev.filter((d) => d.key !== key);
    });

  const send = () =>
    guard(async () => {
      if (!canSend) return;
      setSending(true);
      try {
        const media = drafts.map((d) => ({ id: d.uploaded!.id, alt: d.alt }));
        onPosted(await api.createPost(body, replyTo?.id, media));
        drafts.forEach((d) => URL.revokeObjectURL(d.preview));
        setDrafts([]);
        setBody('');
        toast(replyTo ? 'Reply posted' : 'Posted');
      } finally {
        setSending(false);
      }
    });

  const onPaste = (e: ClipboardEvent) => {
    const files = [...e.clipboardData.files];
    if (files.length) {
      e.preventDefault();
      addFiles(files);
    }
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    addFiles([...e.dataTransfer.files]);
  };

  return (
    <section
      className={`card composer${dragging ? ' dragging' : ''}`}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes('Files')) {
          e.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={onDrop}
    >
      <Avatar username={me.username} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <textarea
          ref={input}
          rows={2}
          placeholder={replyTo ? `Reply to @${replyTo.author.username}…` : "What's on your mind?"}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          onPaste={onPaste}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) send();
          }}
        />

        {drafts.length > 0 && (
          <div className="drafts">
            {drafts.map((d) => (
              <div key={d.key} className={`draft${d.uploaded ? '' : ' uploading'}`}>
                <img src={d.preview} alt="" />
                <button className="draft-remove" aria-label="Remove image" onClick={() => removeDraft(d.key)}>
                  ✕
                </button>
                <input
                  className="draft-alt"
                  placeholder="Alt text"
                  maxLength={420}
                  value={d.alt}
                  aria-label="Image description (alt text)"
                  onChange={(e) =>
                    setDrafts((prev) => prev.map((x) => (x.key === d.key ? { ...x, alt: e.target.value } : x)))
                  }
                />
              </div>
            ))}
          </div>
        )}

        <div className="composer-foot">
          <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <button
              className="icon-btn attach"
              onClick={() => picker.current?.click()}
              disabled={drafts.length >= MAX_IMAGES}
              title="Add images (or paste / drop them)"
            >
              ▣ Image
            </button>
            <input
              ref={picker}
              type="file"
              accept={IMAGE_TYPES.join(',')}
              multiple
              hidden
              onChange={(e) => {
                addFiles([...(e.target.files ?? [])]);
                e.target.value = '';
              }}
            />
            <span className="muted hint" style={{ fontSize: 13 }}>
              <span className="kbd">⌘</span> <span className="kbd">Enter</span> to post
            </span>
          </span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <Ring used={used} />
            <button className="btn" disabled={!canSend} onClick={send}>
              {uploading ? 'Uploading…' : replyTo ? 'Reply' : 'Post'}
            </button>
          </span>
        </div>
      </div>
    </section>
  );
}
