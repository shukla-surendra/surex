// Client for the backend HTTP API (docs/SPEC.md). Requests are same-origin under
// /api/v1; the session travels in the HttpOnly `sid` cookie.

export interface User {
  id: string;
  username: string;
  created_at: number;
}

export interface Post {
  id: string;
  author: User;
  body: string;
  reply_to: string | null;
  created_at: number;
  like_count: number;
  liked: boolean;
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

const enc = encodeURIComponent;
const cursor = (before: string | null) => (before ? `?before=${before}` : '');

export const api = {
  me: () => request<User>('GET', '/me'),
  login: (username: string, password: string) => request<User>('POST', '/auth/login', { username, password }),
  register: (username: string, password: string) => request<User>('POST', '/auth/register', { username, password }),
  logout: () => request<void>('POST', '/auth/logout'),

  homeTimeline: (before: string | null) => request<Page<Post>>('GET', '/timeline/home' + cursor(before)),
  globalTimeline: (before: string | null) => request<Page<Post>>('GET', '/timeline/global' + cursor(before)),
  userPosts: (name: string, before: string | null) =>
    request<Page<Post>>('GET', `/users/${enc(name)}/posts` + cursor(before)),

  createPost: (body: string) => request<Post>('POST', '/posts', { body }),
  deletePost: (id: string) => request<void>('DELETE', `/posts/${id}`),
  setLike: (id: string, on: boolean) => request<void>(on ? 'PUT' : 'DELETE', `/posts/${id}/like`),

  profile: (name: string) => request<Profile>('GET', `/users/${enc(name)}`),
  setFollow: (name: string, on: boolean) => request<void>(on ? 'PUT' : 'DELETE', `/users/${enc(name)}/follow`),
};
