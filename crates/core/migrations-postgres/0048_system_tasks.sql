CREATE TABLE system_task_schedules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    interval_minutes BIGINT NOT NULL CHECK(interval_minutes BETWEEN 5 AND 10080),
    enabled BIGINT NOT NULL CHECK(enabled IN (0,1)),
    next_due_at BIGINT NOT NULL,
    last_started_at BIGINT,
    last_completed_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never' CHECK(last_outcome IN ('never','running','succeeded','failed','interrupted'))
);

INSERT INTO system_task_schedules (id, name, description, category, interval_minutes, enabled, next_due_at) VALUES
('update-extensions', 'Mettre à jour les plugins', 'Vérifie les dépôts distants et actualise les catalogues d''extensions.', 'Extensions', 1440, 1, EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT + 86400),
('optimize-database', 'Optimiser la base de données', 'Exécute l''optimisation du moteur de base de données et des journaux de transactions.', 'Maintenance', 1440, 1, EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT + 86400),
('clean-sessions', 'Nettoyer les sessions expirées', 'Supprime les sessions obsolètes et les jetons d''authentification expirés.', 'Maintenance', 360, 1, EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT + 21600),
('clean-cache', 'Nettoyer le cache et fichiers temporaires', 'Supprime les fragments d''images et fichiers temporaires orphelins.', 'Maintenance', 1440, 1, EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT + 86400),
('clean-playlists', 'Nettoyer les listes et collections', 'Retire des listes de lecture personnelles les éléments dont les médias ont été supprimés.', 'Maintenance', 1440, 1, EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT + 86400);
