-- A Team Manager answers for a department, never the whole workspace (docs/12 §4, owner decision
-- 2026-10-13). Workspace-wide team.manager grants become grants over the grantee's own department;
-- any left (a group grant, or a member with no department) are removed. Pre-release data only.
UPDATE role_grants g SET scope_kind = 'department', scope_id = m.department_id
  FROM roles r, memberships m
 WHERE r.workspace_id = g.workspace_id AND r.id = g.role_id AND r.key = 'team.manager'
   AND g.scope_kind = 'workspace' AND m.workspace_id = g.workspace_id AND m.id = g.membership_id
   AND m.department_id IS NOT NULL;
DELETE FROM role_grants g USING roles r
 WHERE r.workspace_id = g.workspace_id AND r.id = g.role_id AND r.key = 'team.manager' AND g.scope_kind = 'workspace';
