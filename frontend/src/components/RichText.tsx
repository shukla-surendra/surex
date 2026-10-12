import { Fragment } from 'react';

// Highlights @mentions (linked to profiles), #tags and http(s) links in post bodies.
// Everything is rendered as React text nodes, never as HTML.
const TOKEN = /(@\w{3,20}\b|#\w{1,50}\b|https?:\/\/[^\s]+)/g;

export function RichText({ text }: { text: string }) {
  const parts = text.split(TOKEN);
  return (
    <>
      {parts.map((part, i) => {
        if (i % 2 === 0) return <Fragment key={i}>{part}</Fragment>;
        if (part.startsWith('@')) {
          return (
            <a key={i} className="mention" href={`#/${part}`}>
              {part}
            </a>
          );
        }
        if (part.startsWith('#')) {
          return (
            <span key={i} className="tag">
              {part}
            </span>
          );
        }
        return (
          <a key={i} className="link" href={part} target="_blank" rel="noopener noreferrer nofollow">
            {part.replace(/^https?:\/\//, '')}
          </a>
        );
      })}
    </>
  );
}
