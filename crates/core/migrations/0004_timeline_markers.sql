CREATE TABLE manual_timeline_markers (
    asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
    marker_type TEXT NOT NULL CHECK(marker_type IN ('intro','credits')),
    start_seconds REAL,
    end_seconds REAL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    updated_by TEXT NOT NULL REFERENCES users(id),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(asset_id,marker_type),
    CHECK((enabled=0 AND start_seconds IS NULL AND end_seconds IS NULL) OR
          (enabled=1 AND start_seconds>=0 AND end_seconds>start_seconds))
) STRICT;
