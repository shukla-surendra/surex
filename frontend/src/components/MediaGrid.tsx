import { useCallback, useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import type { Media } from '../api';

/** Images attached to a post, laid out by count; click one to open the viewer. */
export function MediaGrid({ media }: { media: Media[] }) {
  const [open, setOpen] = useState<number | null>(null);
  if (media.length === 0) return null;
  const single = media.length === 1 ? media[0] : null;
  return (
    <>
      <div className={`media-grid n${media.length}`}>
        {media.map((m, i) => (
          <button
            key={m.id}
            className="media-cell"
            onClick={() => setOpen(i)}
            aria-label={m.alt ? `Open image: ${m.alt}` : 'Open image'}
            style={single ? { aspectRatio: `${Math.max(m.width / m.height, 0.75)}` } : undefined}
          >
            <img src={m.url} alt={m.alt} width={m.width} height={m.height} loading="lazy" decoding="async" />
            {m.alt && <span className="alt-badge">ALT</span>}
          </button>
        ))}
      </div>
      {open !== null && <Lightbox media={media} start={open} onClose={() => setOpen(null)} />}
    </>
  );
}

function Lightbox({ media, start, onClose }: { media: Media[]; start: number; onClose: () => void }) {
  const [i, setI] = useState(start);
  const step = useCallback((d: number) => setI((n) => (n + d + media.length) % media.length), [media.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
      if (e.key === 'ArrowRight') step(1);
      if (e.key === 'ArrowLeft') step(-1);
    };
    window.addEventListener('keydown', onKey);
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = overflow;
    };
  }, [onClose, step]);

  const m = media[i];
  // Portaled to <body> so card transforms don't affect it; clicks are stopped so they
  // don't reach the post card (which would open the thread).
  return createPortal(
    <div
      className="lightbox"
      role="dialog"
      aria-modal="true"
      aria-label="Image viewer"
      onClick={(e) => {
        e.stopPropagation();
        onClose();
      }}
    >
      <figure onClick={(e) => e.stopPropagation()}>
        <img src={m.url} alt={m.alt} />
        {(m.alt || media.length > 1) && (
          <figcaption>
            {media.length > 1 && <span className="muted">{i + 1} / {media.length}</span>} {m.alt}
          </figcaption>
        )}
      </figure>
      {media.length > 1 && (
        <>
          <button className="lb-nav prev" aria-label="Previous image" onClick={(e) => (e.stopPropagation(), step(-1))}>
            ‹
          </button>
          <button className="lb-nav next" aria-label="Next image" onClick={(e) => (e.stopPropagation(), step(1))}>
            ›
          </button>
        </>
      )}
      <button className="lb-close" aria-label="Close">
        ✕
      </button>
    </div>,
    document.body,
  );
}
