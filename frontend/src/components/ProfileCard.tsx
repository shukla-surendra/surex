import { useEffect, useState } from 'react';
import { api, type Profile, type User } from '../api';
import { nameOf } from '../route';
import { useGuard, useToast } from '../toast';
import { Avatar, tone } from './Avatar';
import { RichText } from './RichText';

const NAME_MAX = 50;
const BIO_MAX = 160;

function EditProfile({ me, onSaved, onCancel }: { me: User; onSaved: (u: User) => void; onCancel: () => void }) {
  const [name, setName] = useState(me.display_name);
  const [bio, setBio] = useState(me.bio);
  const [saving, setSaving] = useState(false);
  const guard = useGuard();
  const toast = useToast();

  const save = () =>
    guard(async () => {
      setSaving(true);
      try {
        onSaved(await api.updateMe(name, bio));
        toast('Profile saved');
      } finally {
        setSaving(false);
      }
    });

  return (
    <form
      className="edit-profile"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <label>
        <span className="label-row">
          Display name <span className="muted">{[...name].length}/{NAME_MAX}</span>
        </span>
        <input className="field" value={name} maxLength={NAME_MAX} placeholder={me.username} onChange={(e) => setName(e.target.value)} />
      </label>
      <label>
        <span className="label-row">
          Bio <span className="muted">{[...bio].length}/{BIO_MAX}</span>
        </span>
        <textarea
          className="field"
          rows={3}
          value={bio}
          maxLength={BIO_MAX}
          placeholder="A line or two about you"
          onChange={(e) => setBio(e.target.value)}
        />
      </label>
      <div className="auth-row" style={{ justifyContent: 'flex-end' }}>
        <button type="button" className="btn ghost" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn" type="submit" disabled={saving}>
          Save
        </button>
      </div>
    </form>
  );
}

export function ProfileCard({ name, me, onMeChanged }: { name: string; me: User | null; onMeChanged: (u: User) => void }) {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [editing, setEditing] = useState(false);
  const guard = useGuard();

  useEffect(() => {
    guard(async () => setProfile(await api.profile(name)));
  }, [name, me?.id, guard]);

  const base = tone(name);
  const banner = {
    background: `radial-gradient(120% 140% at 0% 0%, ${base}, transparent 60%),
                 radial-gradient(120% 140% at 100% 100%, color-mix(in srgb, ${base} 45%, var(--accent)), transparent 55%),
                 var(--surface-2)`,
  };

  if (!profile) {
    return (
      <section className="card profile">
        <div className="profile-banner" style={banner} />
        <div className="profile-main" style={{ height: 110 }} />
      </section>
    );
  }

  const isMe = !!me && me.id === profile.id;
  const toggleFollow = () =>
    guard(async () => {
      const on = !profile.followed_by_me;
      await api.setFollow(profile.username, on);
      setProfile({ ...profile, followed_by_me: on, followers: profile.followers + (on ? 1 : -1) });
    });
  const onSaved = (u: User) => {
    setProfile({ ...profile, display_name: u.display_name, bio: u.bio });
    setEditing(false);
    onMeChanged(u);
  };

  const joined = new Date(profile.created_at).toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  const base_ = `#/@${encodeURIComponent(profile.username)}`;

  return (
    <section className="card profile">
      <div className="profile-banner" style={banner} />
      <div className="profile-main">
        <div className="profile-top">
          <Avatar username={profile.username} size={84} />
          {isMe && !editing && (
            <button className="btn ghost" onClick={() => setEditing(true)}>
              Edit profile
            </button>
          )}
          {me && !isMe && (
            <button className={profile.followed_by_me ? 'btn moss' : 'btn'} onClick={toggleFollow}>
              {profile.followed_by_me ? '✓ Following' : 'Follow'}
            </button>
          )}
        </div>

        {editing && me ? (
          <EditProfile me={{ ...me, display_name: profile.display_name, bio: profile.bio }} onSaved={onSaved} onCancel={() => setEditing(false)} />
        ) : (
          <>
            <div className="profile-name">{nameOf(profile)}</div>
            <div className="muted">
              @{profile.username} · Joined {joined}
            </div>
            {profile.bio && (
              <p className="profile-bio">
                <RichText text={profile.bio} />
              </p>
            )}
          </>
        )}

        <div className="stats">
          <span>
            <b>{profile.posts}</b>
            <span className="muted">posts</span>
          </span>
          <a href={`${base_}/followers`}>
            <b>{profile.followers}</b>
            <span className="muted">followers</span>
          </a>
          <a href={`${base_}/following`}>
            <b>{profile.following}</b>
            <span className="muted">following</span>
          </a>
        </div>
      </div>
    </section>
  );
}
