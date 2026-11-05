-- Native application extensions (Signalements, Demandes) serve every library the
-- caller may already read. Installs made before that default left them with an
-- empty scope (no library, no grant), which refused every report and request
-- with library_permission_denied. Idempotent: rows with explicit grants or an
-- all-libraries scope are untouched.
UPDATE extensions SET all_libraries=1
WHERE all_libraries=0
  AND id IN ('media-reports','media-requests')
  AND NOT EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=extensions.id);
