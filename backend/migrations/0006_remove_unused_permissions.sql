-- Remove demo-only permissions that have no API authorization or frontend guard consumer.
-- Deleting permissions also removes their role assignments through ON DELETE CASCADE.
DELETE FROM permissions
WHERE code IN (
    'dashboard:analytics:export',
    'dashboard:widgets:manage',
    'resource:hw:read',
    'resource:infra:manage',
    'resource:license:grant',
    'audit:logs:read',
    'audit:logs:export',
    'security:policies:manage',
    'security:mfa:enforce',
    'security:threats:read'
);

DELETE FROM permission_groups
WHERE code IN ('resources', 'audit', 'security');
