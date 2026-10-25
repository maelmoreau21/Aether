-- Add cover_path, cover_size, cover_modified_ns, cover_mime to music_tracks for album art
ALTER TABLE music_tracks ADD COLUMN cover_path TEXT;
ALTER TABLE music_tracks ADD COLUMN cover_size INTEGER;
ALTER TABLE music_tracks ADD COLUMN cover_modified_ns TEXT;
ALTER TABLE music_tracks ADD COLUMN cover_mime TEXT;
