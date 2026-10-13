//! Request comments, @mentions and watchers (BPMS-214, FR-R02, FX-R02). A comment is append-only.
//! `internal` comments are hidden from the person who started the request, who also can't post
//! one. Mentions name members the caller has already checked can see the request (the HTTP layer
//! runs the access check per person); they hear `request_mention`. Everyone else following the
//! request — the requester (unless internal), the people on its open steps and its watchers —
//! hears `request_comment`. The author is never told about their own comment. Audited
//! `request.commented`; watching is `request.watched` / `request.unwatched`.

use std::collections::BTreeSet;

use procera_db::WsTx;
use procera_notify::delivery::Urgency;
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::views::Person;
use crate::{Actor, FieldError, RequestError, Requests};

type R<T> = Result<T, RequestError>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Comment {
    pub id: Uuid,
    pub author: Person,
    pub body: String,
    pub internal: bool,
    pub mentions: Vec<Person>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Conversation {
    pub comments: Vec<Comment>,
    /// Whether the caller watches the request.
    pub watching: bool,
    pub watchers: Vec<Person>,
}

/// What someone asks to post.
#[derive(Debug, Clone, Default)]
pub struct NewComment {
    pub body: String,
    pub internal: bool,
    /// Members to mention; each must already be able to see the request (checked by the caller).
    pub mentions: Vec<Uuid>,
}

type Row = (Uuid, Uuid, String, String, bool, Vec<Uuid>, OffsetDateTime);

impl Requests {
    /// The conversation as `viewer` may see it (internal comments hidden from the requester).
    pub async fn conversation(&self, tx: &mut WsTx<'_>, id: Uuid, viewer: Uuid) -> R<Conversation> {
        let requester = self.initiator(tx, id).await?;
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT c.id, c.author_membership_id, u.display_name, c.body, c.internal, c.mentions, c.created_at
               FROM request_comments c JOIN memberships m ON m.workspace_id = c.workspace_id AND m.id = c.author_membership_id
               JOIN users u ON u.id = m.user_id
              WHERE c.instance_id = $1 AND (NOT c.internal OR $2) ORDER BY c.created_at, c.id",
        )
        .bind(id)
        .bind(viewer != requester)
        .fetch_all(tx.conn())
        .await?;
        let mut comments = Vec::with_capacity(rows.len());
        for r in rows {
            let mut mentions = Vec::new();
            for m in &r.5 {
                mentions.push(Person {
                    id: *m,
                    name: crate::handler::name(tx, *m).await.map_err(RequestError::Internal)?,
                });
            }
            comments.push(Comment {
                id: r.0,
                author: Person { id: r.1, name: r.2 },
                body: r.3,
                internal: r.4,
                mentions,
                created_at: r.6,
            });
        }
        let watchers: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT w.membership_id, u.display_name FROM request_watchers w
               JOIN memberships m ON m.workspace_id = w.workspace_id AND m.id = w.membership_id JOIN users u ON u.id = m.user_id
              WHERE w.instance_id = $1 ORDER BY u.display_name",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        Ok(Conversation {
            watching: watchers.iter().any(|w| w.0 == viewer),
            watchers: watchers.into_iter().map(|(id, name)| Person { id, name }).collect(),
            comments,
        })
    }

    /// Posts a comment and tells the people following the request.
    pub async fn comment(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid, c: &NewComment) -> R<Comment> {
        let requester = self.initiator(tx, id).await?;
        let body = c.body.trim();
        let mut errors = Vec::new();
        if body.is_empty() || body.chars().count() > 4000 {
            errors.push(FieldError::new("body", "length", "Write a comment of up to 4000 characters."));
        }
        if c.internal && actor.membership == requester {
            errors.push(FieldError::new(
                "internal",
                "requester",
                "The person who started the request can’t post internal notes.",
            ));
        }
        let mentions: Vec<Uuid> = c
            .mentions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|m| *m != actor.membership)
            .collect();
        if c.internal && mentions.contains(&requester) {
            errors.push(FieldError::new(
                "mentions",
                "requester_internal",
                "An internal note is hidden from the requester, so they can’t be mentioned in it.",
            ));
        }
        if mentions.len() > 20 {
            errors.push(FieldError::new("mentions", "too_many", "Mention up to 20 people."));
        }
        if !errors.is_empty() {
            return Err(RequestError::Invalid(errors));
        }
        let cid = self.ids.new_id();
        let now = self.clock.now();
        sqlx::query(
            "INSERT INTO request_comments (workspace_id, id, instance_id, author_membership_id, body, internal, mentions, created_at)
             VALUES (current_setting('app.workspace_id')::uuid, $1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(cid)
        .bind(id)
        .bind(actor.membership)
        .bind(body)
        .bind(c.internal)
        .bind(&mentions)
        .bind(now)
        .execute(tx.conn())
        .await?;
        sqlx::query("UPDATE requests SET updated_at = $2 WHERE instance_id = $1")
            .bind(id)
            .bind(now)
            .execute(tx.conn())
            .await?;
        let author = crate::handler::name(tx, actor.membership).await.map_err(RequestError::Internal)?;
        let (display_id, title): (String, String) =
            sqlx::query_as("SELECT display_id, title FROM requests WHERE instance_id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
        let excerpt: String = if body.chars().count() > 160 {
            format!("{}…", body.chars().take(157).collect::<String>())
        } else {
            body.to_owned()
        };
        let vars = json!({ "display_id": display_id, "title": title, "author": author, "excerpt": excerpt, "internal": c.internal });
        let link = format!("/requests/{id}#comment-{cid}");
        for m in &mentions {
            let dedupe = format!("request_mention:{cid}:{m}");
            self.effects
                .tell(tx, *m, "request_mention", &vars, &link, ("request", id), &dedupe, Urgency::Normal)
                .await
                .map_err(RequestError::Internal)?;
        }
        let mut followers: BTreeSet<Uuid> = sqlx::query_scalar::<_, Uuid>(
            "SELECT c.membership_id FROM work_items w JOIN work_item_candidates c ON c.workspace_id = w.workspace_id AND c.work_item_id = w.id
              WHERE w.instance_id = $1 AND w.state IN ('offered', 'claimed')
             UNION SELECT membership_id FROM request_watchers WHERE instance_id = $1",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?
        .into_iter()
        .collect();
        if !c.internal {
            followers.insert(requester);
        } else {
            followers.remove(&requester);
        }
        followers.remove(&actor.membership);
        for m in followers.difference(&mentions.iter().copied().collect()) {
            let dedupe = format!("request_comment:{cid}:{m}");
            self.effects
                .tell(tx, *m, "request_comment", &vars, &link, ("request", id), &dedupe, Urgency::Normal)
                .await
                .map_err(RequestError::Internal)?;
        }
        let diff = json!({ "comment": cid, "internal": c.internal, "mentions": mentions, "length": body.chars().count() });
        self.audit(tx, actor, "request.commented", "request", id, diff).await?;
        let mut people = Vec::new();
        for m in &mentions {
            people.push(Person {
                id: *m,
                name: crate::handler::name(tx, *m).await.map_err(RequestError::Internal)?,
            });
        }
        Ok(Comment {
            id: cid,
            author: Person { id: actor.membership, name: author },
            body: body.to_owned(),
            internal: c.internal,
            mentions: people,
            created_at: now,
        })
    }

    /// Starts or stops watching a request. Idempotent: watching twice keeps one row (no new audit).
    pub async fn watch(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid, on: bool) -> R<bool> {
        self.initiator(tx, id).await?;
        let changed = if on {
            sqlx::query(
                "INSERT INTO request_watchers (workspace_id, instance_id, membership_id, created_at)
                 VALUES (current_setting('app.workspace_id')::uuid, $1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(actor.membership)
            .bind(self.clock.now())
            .execute(tx.conn())
            .await?
            .rows_affected()
        } else {
            sqlx::query("DELETE FROM request_watchers WHERE instance_id = $1 AND membership_id = $2")
                .bind(id)
                .bind(actor.membership)
                .execute(tx.conn())
                .await?
                .rows_affected()
        };
        if changed > 0 {
            let action = if on { "request.watched" } else { "request.unwatched" };
            self.audit(tx, actor, action, "request", id, json!({})).await?;
        }
        Ok(on)
    }
}
