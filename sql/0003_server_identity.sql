ALTER TABLE servers ADD COLUMN socket_path TEXT;
ALTER TABLE servers ADD COLUMN start_time INTEGER;

UPDATE servers
SET socket_path = server_key
WHERE socket_path IS NULL;

UPDATE servers
SET start_time = CAST(created_at AS INTEGER)
WHERE start_time IS NULL;

CREATE INDEX IF NOT EXISTS idx_servers_socket_path ON servers(socket_path);
