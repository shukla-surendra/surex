import { useEffect, useState } from 'react';
import { api, type Profile, type User } from '../api';
import { useGuard } from '../toast';

export function ProfileCard({ name, me }: { name: string; me: User | null }) {
  const [profile, setProfile] = useState<Profile | null>(null);
  const guard = useGuard();

  useEffect(() => {
    guard(async () => setProfile(await api.profile(name)));
  }, [name, me?.id, guard]);

  if (!profile) return <section className="box" />;

  const toggleFollow = () =>
    guard(async () => {
      const on = !profile.followed_by_me;
      await api.setFollow(profile.username, on);
      setProfile({ ...profile, followed_by_me: on, followers: profile.followers + (on ? 1 : -1) });
    });

  return (
    <section className="box">
      <div className="row">
        <div>
          <div className="pn">@{profile.username}</div>
          <div className="m">
            {profile.followers} followers · {profile.following} following · {profile.posts} posts
          </div>
        </div>
        {me && me.id !== profile.id && (
          <button className={profile.followed_by_me ? 'o' : ''} onClick={toggleFollow}>
            {profile.followed_by_me ? 'Unfollow' : 'Follow'}
          </button>
        )}
      </div>
    </section>
  );
}
