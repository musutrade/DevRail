-- 种子管理员：admin / admin123（argon2id 真实哈希，由 tools 生成，非明文）
-- 账号首次登录后请在正式环境尽快修改密码

INSERT INTO users (username, password_hash, display_name, email, status)
VALUES (
    'admin',
    '$argon2id$v=19$m=19456,t=2,p=1$pDDhKh46fVQNqRy3OeXTTw$+5qvGkvmKsilvsRWsskXT4k6fmmE4q35ntz6ME1UNBE',
    'Administrator',
    'admin@example.com',
    'active'
);

INSERT INTO user_roles (user_id, role_id)
SELECT u.id, r.id
FROM users u
JOIN roles r ON r.code = 'super_admin'
WHERE u.username = 'admin';
