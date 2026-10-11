-- BPMS-045 (FR-PL01/PL02): policy category and applicability; restricted policies are hidden from
-- readers they do not apply to.

ALTER TABLE artifacts
  ADD COLUMN category text CHECK (category IS NULL OR length(category) BETWEEN 1 AND 60),
  ADD COLUMN applicability jsonb CHECK (applicability IS NULL OR jsonb_typeof(applicability) = 'object'), -- Audience; NULL = everyone
  ADD COLUMN restricted boolean NOT NULL DEFAULT false;

-- Whether a membership is in an audience ({all, departments[], groups[], roles[]}); the SQL twin of
-- procera_members::audience::contains, for filtering lists in one statement. Runs as the caller,
-- so RLS applies.
CREATE FUNCTION procera_audience_contains(aud jsonb, membership uuid) RETURNS boolean
LANGUAGE sql STABLE AS $$
  SELECT EXISTS (
    SELECT 1 FROM memberships m
    WHERE m.id = membership AND m.status = 'active' AND (
      coalesce((aud->>'all')::boolean, false)
      OR EXISTS (SELECT 1 FROM departments d WHERE d.id = m.department_id
                 AND d.path && ARRAY(SELECT jsonb_array_elements_text(coalesce(aud->'departments', '[]')))::uuid[])
      OR EXISTS (SELECT 1 FROM group_members gm WHERE gm.membership_id = m.id
                 AND gm.group_id = ANY(ARRAY(SELECT jsonb_array_elements_text(coalesce(aud->'groups', '[]')))::uuid[]))
      OR EXISTS (SELECT 1 FROM role_grants g JOIN roles r ON r.workspace_id = g.workspace_id AND r.id = g.role_id
                 WHERE r.key = ANY(ARRAY(SELECT jsonb_array_elements_text(coalesce(aud->'roles', '[]'))))
                   AND (g.expires_at IS NULL OR g.expires_at > now())
                   AND (g.membership_id = m.id OR g.group_id IN (SELECT gm2.group_id FROM group_members gm2 WHERE gm2.membership_id = m.id)))))
$$;
