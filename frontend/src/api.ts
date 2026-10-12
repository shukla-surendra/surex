// Client for the backend HTTP API (docs/SPEC.md). Requests are same-origin under
// /api/v1; the session travels in the HttpOnly `sid` cookie.

export interface User {
  id: string;
  username: string;
  created_at: number;
  /** Empty when unset; show the username instead. */
  display_name: string;
  bio: string;
}

export interface Media {
  id: string;
  url: string;
  content_type: string;
  width: number;
  height: number;
  alt: string;
}

export interface Post {
  id: string;
  author: User;
  body: string;
  reply_to: string | null;
  /** Username of the parent post's author, for replies. */
  replying_to: string | null;
  created_at: number;
  like_count: number;
  reply_count: number;
  liked: boolean;
  media: Media[];
}

export interface Profile extends User {
  followers: number;
  following: number;
  posts: number;
  followed_by_me: boolean;
}

export interface Page<T> {
  items: T[];
  next: string | null;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch('/api/v1' + path, {
    method,
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 204) return undefined as T;
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(json.error?.message ?? `HTTP ${res.status}`);
  return json as T;
}

/** Uploads one image; the server detects its type from the bytes. */
async function uploadMedia(file: Blob): Promise<Media> {
  const res = await fetch('/api/v1/media', { method: 'POST', headers: { 'content-type': file.type }, body: file });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(json.error?.message ?? (res.status === 413 ? 'Image is too large (max 5 MB)' : `HTTP ${res.status}`));
  return json as Media;
}

const enc = encodeURIComponent;
const cursor = (before: string | null) => (before ? `?before=${before}` : '');
const after = (id: string | null) => (id ? `?after=${id}` : '');

export const api = {
  me: () => request<User>('GET', '/me'),
  login: (username: string, password: string) => request<User>('POST', '/auth/login', { username, password }),
  register: (username: string, password: string) => request<User>('POST', '/auth/register', { username, password }),
  logout: () => request<void>('POST', '/auth/logout'),
  updateMe: (display_name: string, bio: string) => request<User>('PATCH', '/me', { display_name, bio }),

  homeTimeline: (before: string | null) => request<Page<Post>>('GET', '/timeline/home' + cursor(before)),
  globalTimeline: (before: string | null) => request<Page<Post>>('GET', '/timeline/global' + cursor(before)),
  userPosts: (name: string, before: string | null) =>
    request<Page<Post>>('GET', `/users/${enc(name)}/posts` + cursor(before)),

  createPost: (body: string, reply_to?: string, media: { id: string; alt: string }[] = []) =>
    request<Post>('POST', '/posts', { body, reply_to, media }),
  uploadMedia,
  getPost: (id: string) => request<Post>('GET', `/posts/${id}`),
  replies: (id: string, from: string | null) => request<Page<Post>>('GET', `/posts/${id}/replies` + after(from)),
  deletePost: (id: string) => request<void>('DELETE', `/posts/${id}`),
  setLike: (id: string, on: boolean) => request<void>(on ? 'PUT' : 'DELETE', `/posts/${id}/like`),

  profile: (name: string) => request<Profile>('GET', `/users/${enc(name)}`),
  setFollow: (name: string, on: boolean) => request<void>(on ? 'PUT' : 'DELETE', `/users/${enc(name)}/follow`),
  followers: (name: string, from: string | null) =>
    request<Page<User>>('GET', `/users/${enc(name)}/followers` + after(from)),
  following: (name: string, from: string | null) =>
    request<Page<User>>('GET', `/users/${enc(name)}/following` + after(from)),
};
