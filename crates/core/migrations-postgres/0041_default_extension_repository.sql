-- Register once. Deleting this administrator-owned row survives every restart.
-- No module installation, publisher approval, or network request during migration.
INSERT INTO extension_repositories(id,name,url,manifest_json,refreshed_at)
VALUES('community-default','Aether Plugins','https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json', '{"apiVersion":1,"extensions":[]}',0)
ON CONFLICT(url) DO NOTHING;
