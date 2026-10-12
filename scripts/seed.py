#!/usr/bin/env python3
"""Load demo fixtures (users, follows, posts, replies, likes) through the HTTP API.

Works against any backend (SQLite or Scylla) because it only uses /api/v1. Fixtures
load in order; by default `demo.json`, `pagination.json`, then `media.json`. Images
named in a fixture are read from paths relative to that fixture file.

Safe to re-run:
  * existing users are logged in instead of registered; follows and likes are idempotent;
  * a hand-written post is only created if its author has no posts yet, or, for a
    fixture with a `once` marker, only if the marker post does not exist yet;
  * a fixture's `bulk` section is skipped entirely if its marker user already exists.

    python3 scripts/seed.py                                  # API on http://127.0.0.1:3000
    python3 scripts/seed.py --api http://localhost:8080      # through the frontend's proxy
    python3 scripts/seed.py fixtures/demo.json               # just one fixture

The bulk fixture makes a few hundred writes. With the API's rate limits on, that takes
several minutes; start the backend with SOCIAL_NO_RATE_LIMIT=true to make it instant.

Demo data only: never run this against a production deployment.
"""

import argparse
import http.cookiejar
import json
import mimetypes
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_FIXTURES = [ROOT / "fixtures" / name for name in ("demo.json", "pagination.json", "media.json")]

throttle_hint_shown = False


class Client:
    """One logged-in user: keeps its own session cookie."""

    def __init__(self, api: str):
        self.api = api.rstrip("/") + "/api/v1"
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def upload(self, path: Path) -> str:
        """Uploads an image file and returns its media id."""
        content_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        return ok(*self.call("POST", "/media", path.read_bytes(), content_type), f"upload {path.name}")["id"]

    def call(self, method: str, path: str, body=None, content_type: str = "application/json"):
        global throttle_hint_shown
        if isinstance(body, bytes):
            data = body
        else:
            data = None if body is None else json.dumps(body).encode()
        headers = {} if data is None else {"content-type": content_type}
        for _ in range(60):
            req = urllib.request.Request(self.api + path, data=data, method=method, headers=headers)
            try:
                with self.opener.open(req) as res:
                    raw = res.read()
                    return res.status, (json.loads(raw) if raw else None)
            except urllib.error.HTTPError as e:
                if e.code == 429:  # rate limited: wait as told, then retry
                    if not throttle_hint_shown:
                        throttle_hint_shown = True
                        print("  (rate limited; waiting. For instant seeding restart the backend with "
                              "SOCIAL_NO_RATE_LIMIT=true)")
                    time.sleep(int(e.headers.get("Retry-After", "1")))
                    continue
                raw = e.read()
                return e.code, (json.loads(raw) if raw else None)
        sys.exit(f"gave up on {method} {path}: still rate limited")


def ok(status: int, body, what: str):
    if status >= 300:
        sys.exit(f"{what} failed: HTTP {status} {body}")
    return body


class Seeder:
    def __init__(self, api: str, password: str):
        self.api = api
        self.password = password
        self.clients: dict[str, Client] = {}
        self.base = ROOT / "fixtures"  # images resolve relative to the current fixture
        self.uploads = 0

    def media(self, author: str, images: list[dict], n: int | None = None) -> list[dict]:
        """Uploads `images` ([{file, alt}]) as `author`; returns the post's `media` list."""
        out = []
        for img in images:
            alt = img.get("alt", "")
            out.append({"id": self.user(author).upload(self.base / img["file"]), "alt": alt.format(n=n) if n else alt})
            self.uploads += 1
        return out

    def ensure_user(self, name: str, display_name: str = "", bio: str = "") -> bool:
        """Registers (or logs in) `name` and sets its profile. Returns True if created."""
        c = Client(self.api)
        status, body = c.call("POST", "/auth/register", {"username": name, "password": self.password})
        created = status != 409
        if created:
            ok(status, body, f"register {name}")
        else:
            ok(*c.call("POST", "/auth/login", {"username": name, "password": self.password}), f"login {name}")
        ok(*c.call("PATCH", "/me", {"display_name": display_name, "bio": bio}), f"profile {name}")
        self.clients[name] = c
        return created

    def user(self, name: str) -> Client:
        """A logged-in client for a user created by this or an earlier fixture."""
        if name not in self.clients:
            c = Client(self.api)
            ok(*c.call("POST", "/auth/login", {"username": name, "password": self.password}),
               f"login {name} (load the fixture that defines it first)")
            self.clients[name] = c
        return self.clients[name]

    def post_count(self, name: str) -> int:
        return ok(*self.user(name).call("GET", f"/users/{name}"), f"profile {name}")["posts"]

    # ---- hand-written fixture sections ---------------------------------------------

    def load(self, fx: dict):
        for u in fx.get("users", []):
            created = self.ensure_user(u["username"], u.get("display_name", ""), u.get("bio", ""))
            print(f"  user  {u['username']} ({'created' if created else 'exists, logged in'})")
        if "once" in fx:
            # Create every post unless the marker post exists already.
            done = self.find_post(fx["once"]["author"], fx["once"]["starts_with"], required=False)
            fresh = set() if done else {p["author"] for p in fx.get("posts", [])}
        else:
            fresh = {u["username"] for u in fx.get("users", []) if self.post_count(u["username"]) == 0}

        for follower, followee in fx.get("follows", []):
            ok(*self.user(follower).call("PUT", f"/users/{followee}/follow"), f"{follower} follows {followee}")
        if fx.get("follows"):
            print(f"  {len(fx['follows'])} follows")

        ids: dict[str, str] = {}
        for p in fx.get("posts", []):
            if p["author"] not in fresh:
                continue
            body = {"body": p["body"], "media": self.media(p["author"], p.get("images", []))}
            if "reply_to" in p:
                if p["reply_to"] not in ids:
                    continue  # parent was not created in this run
                body["reply_to"] = ids[p["reply_to"]]
            ids[p["key"]] = ok(*self.user(p["author"]).call("POST", "/posts", body), f"post {p['key']}")["id"]
        if fx.get("posts"):
            with_images = sum(1 for p in fx["posts"] if p["key"] in ids and p.get("images"))
            print(f"  {len(ids)} posts and replies ({with_images} with images)" if ids else "  posts already loaded; skipped")

        liked = [(u, k) for u, k in fx.get("likes", []) if k in ids]
        for user, key in liked:
            ok(*self.user(user).call("PUT", f"/posts/{ids[key]}/like"), f"{user} likes {key}")
        if fx.get("likes"):
            print(f"  {len(liked)} likes")

        if "bulk" in fx:
            self.bulk(fx["bulk"])

    # ---- generated bulk section --------------------------------------------------------

    def bulk(self, spec: dict):
        probe = Client(self.api).call("GET", f"/users/{spec['marker']}")
        if probe[0] == 200:
            print(f"  bulk data already loaded (user {spec['marker']} exists); skipped")
            return

        crowd = spec["crowd"]
        names = []
        for n in range(1, crowd["count"] + 1):
            name = f"{crowd['prefix']}{n:02d}"
            first = crowd["first_names"][(n - 1) % len(crowd["first_names"])]
            last = crowd["last_initials"][(n - 1) // len(crowd["first_names"]) % len(crowd["last_initials"])]
            self.ensure_user(name, f"{first} {last}", crowd["bios"][(n - 1) % len(crowd["bios"])])
            for target in crowd.get("follow", []):
                ok(*self.user(name).call("PUT", f"/users/{target}/follow"), f"{name} follows {target}")
            names.append(name)
        print(f"  {len(names)} crowd users, each following {', '.join(crowd.get('follow', [])) or 'nobody'}")

        for batch in spec.get("posts", []):
            pics = batch.get("images")
            for n in range(1, batch["count"] + 1):
                body = {"body": render(batch, n), "media": []}
                if pics and n % pics["every"] == pics.get("offset", 0) % pics["every"]:
                    file = pics["files"][(n // pics["every"]) % len(pics["files"])]
                    body["media"] = self.media(batch["author"], [{"file": file, "alt": pics.get("alt", "")}], n)
                ok(*self.user(batch["author"]).call("POST", "/posts", body), f"bulk post by {batch['author']}")
            print(f"  {batch['count']} generated posts by {batch['author']}")

        if "thread" in spec:
            t = spec["thread"]
            root = self.find_post(t["author"], t["starts_with"])
            for n in range(1, t["count"] + 1):
                replier = names[(n - 1) % len(names)]
                ok(*self.user(replier).call("POST", "/posts", {"body": render(t, n), "reply_to": root}), "bulk reply")
            print(f"  {t['count']} replies on @{t['author']}'s \"{t['starts_with']}…\" post")

    def find_post(self, author: str, starts_with: str, required: bool = True) -> str | None:
        before = None
        while True:
            q = f"?limit=100&before={before}" if before else "?limit=100"
            page = ok(*self.user(author).call("GET", f"/users/{author}/posts{q}"), f"posts of {author}")
            for p in page["items"]:
                if p["reply_to"] is None and p["body"].startswith(starts_with):
                    return p["id"]
            if not page["next"]:
                if not required:
                    return None
                sys.exit(f"no post by {author} starting with {starts_with!r}; load demo.json first")
            before = page["next"]


def render(spec: dict, n: int) -> str:
    """Fills template `n` (1-based); `{n}` is the number, `{key}` cycles through words[key]."""
    template = spec["templates"][(n - 1) % len(spec["templates"])]
    values = {"n": n}
    for i, (key, options) in enumerate(spec.get("words", {}).items()):
        values[key] = options[(n * (i + 3) + i) % len(options)]
    return template.format(**values)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("fixtures", nargs="*", help="fixture files, loaded in order (default: demo + pagination)")
    ap.add_argument("--api", default="http://127.0.0.1:3000", help="backend base URL")
    args = ap.parse_args()
    paths = [Path(p) for p in args.fixtures] or DEFAULT_FIXTURES

    try:
        urllib.request.urlopen(args.api.rstrip("/") + "/api/v1/timeline/global", timeout=5)
    except Exception as e:  # noqa: BLE001 - any failure means the API is not reachable
        sys.exit(f"cannot reach the API at {args.api}: {e}\nStart it first (see README).")

    seeder = None
    for path in paths:
        fx = json.loads(path.read_text())
        print(f"{path.name}:")
        if seeder is None or seeder.password != fx["password"]:
            seeder = Seeder(args.api, fx["password"])
        seeder.base = path.resolve().parent
        seeder.load(fx)
    print(f"done ({seeder.uploads} images uploaded). Log in as any fixture user with password: {seeder.password}")


if __name__ == "__main__":
    main()
