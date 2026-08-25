ALTER TABLE devrail_outbox_events
  DROP CONSTRAINT IF EXISTS devrail_outbox_events_organization_id_event_type_aggregate_type_aggregate_id_key;

WITH ranked AS (
  SELECT id,
         ROW_NUMBER() OVER (
           PARTITION BY organization_id, event_type, aggregate_type, aggregate_id,
             COALESCE(payload->>'notificationSource', payload::text)
           ORDER BY id
         ) AS row_number
  FROM devrail_outbox_events
)
DELETE FROM devrail_outbox_events events
USING ranked
WHERE events.id = ranked.id AND ranked.row_number > 1;

CREATE UNIQUE INDEX devrail_outbox_events_dedup_idx
  ON devrail_outbox_events (
    organization_id,
    event_type,
    aggregate_type,
    aggregate_id,
    COALESCE(payload->>'notificationSource', payload::text)
  );
