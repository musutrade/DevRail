-- Disable the historical demo administrator unless its password was already changed.
-- Production bootstrap is handled explicitly by the bootstrap_admin binary.
UPDATE users
SET status = 'inactive',
    password_hash = '$disabled-default-admin$',
    updated_at = now()
WHERE username = 'admin'
  AND password_hash = '$argon2id$v=19$m=19456,t=2,p=1$pDDhKh46fVQNqRy3OeXTTw$+5qvGkvmKsilvsRWsskXT4k6fmmE4q35ntz6ME1UNBE';
