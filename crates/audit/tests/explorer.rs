//! BPMS-064: explorer filters, keyset pagination (model-based), view-own scoping, actor names,
//! timelines, saved queries and audited exports.

use procera_audit::explorer::{
    self, EXPORT_ROW_CAP, ExplorerError, ExportFormat, Filter, Who, delete_saved_query, export, query,
    save_query, saved_queries, timeline,
};
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append, verify};
use procera_db::{WsCtx, WsTx, test_support};
use procera_kernel::clock::ManualClock;
use procera_kernel::{Clock, UuidV7Gen};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

macro_rules! db_or_skip {
    () => {
        match test_support::fresh().await {
            Some(r) => r?,
            None => return Ok(()),
        }
    };
}

/// A member of the workspace with a known audit subject.
async fn member(tx: &mut WsTx<'_>, name: &str) -> anyhow::Result<Who> {
    let (user, subject) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(name)
        .bind(subject)
        .execute(tx.conn())
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .execute(tx.conn())
        .await?;
    Ok(Who { membership: m, subject_ref: subject, ip: Some("10.0.0.1".into()), user_agent: None })
}

/// Tiny deterministic generator, so the model test is reproducible without extra dependencies.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

const ACTIONS: &[(&str, Category)] = &[
    ("member.invited", Category::Access),
    ("member.deactivated", Category::Access),
    ("role.created", Category::Access),
    ("version.published", Category::Content),
    ("setting.changed", Category::Config),
];

#[derive(Debug, Clone)]
struct Model {
    seq: i64,
    at: OffsetDateTime,
    actor: Uuid,
    action: &'static str,
    category: &'static str,
    object: Uuid,
    result: &'static str,
}

#[tokio::test]
async fn filters_and_keyset_pages_match_a_model() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let clock = ManualClock::new(OffsetDateTime::now_utc());
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    let people = [member(&mut tx, "Ada").await?, member(&mut tx, "Ben").await?, member(&mut tx, "Cy").await?];
    let objects = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
    let mut rng = Lcg(7);
    let mut model = vec![];
    for _ in 0..240 {
        clock.advance(Duration::seconds(1 + rng.next(90) as i64));
        let (action, cat) = ACTIONS[rng.next(ACTIONS.len() as u64) as usize];
        let who = &people[rng.next(3) as usize];
        let result = if rng.next(5) == 0 { Outcome::Denied } else { Outcome::Allowed };
        let object = objects[rng.next(4) as usize];
        let ev = AuditEvent::new(Some(who.subject_ref), ActorKind::User, cat, action, result)
            .object("thing", object);
        let r = append(&mut tx, &clock, &UuidV7Gen, &ev).await?;
        model.push(Model {
            seq: r.seq,
            at: clock.now(),
            actor: who.subject_ref,
            action,
            category: match cat {
                Category::Access => "access",
                Category::Content => "content",
                _ => "config",
            },
            object,
            result: if result == Outcome::Denied { "denied" } else { "allowed" },
        });
    }
    // An event from a non-member: no name is resolved.
    let stranger = Uuid::now_v7();
    clock.advance(Duration::seconds(1));
    append(
        &mut tx,
        &clock,
        &UuidV7Gen,
        &AuditEvent::new(
            Some(stranger),
            ActorKind::User,
            Category::Auth,
            "login.succeeded",
            Outcome::Allowed,
        ),
    )
    .await?;
    tx.commit().await?;

    let mid = model[model.len() / 3].at;
    let late = model[model.len() * 2 / 3].at;
    let filters = [
        Filter::default(),
        Filter { action: Some("member.".into()), ..Filter::default() },
        Filter { category: Some("content".into()), result: Some("allowed".into()), ..Filter::default() },
        Filter { actor: Some(people[1].subject_ref), ..Filter::default() },
        Filter { object_type: Some("thing".into()), object_id: Some(objects[2]), ..Filter::default() },
        Filter { from: Some(mid), to: Some(late), ..Filter::default() },
        Filter { action: Some("role.created".into()), from: Some(mid), ..Filter::default() },
    ];
    let mut tx = t.db.begin_ws(ws).await?;
    for (i, f) in filters.iter().enumerate() {
        for own in [None, Some(people[0].subject_ref)] {
            let expected: Vec<i64> = model
                .iter()
                .rev()
                .filter(|m| {
                    f.action.as_deref().is_none_or(|p| m.action.starts_with(p))
                        && f.category.as_deref().is_none_or(|c| m.category == c)
                        && f.result.as_deref().is_none_or(|r| m.result == r)
                        && f.actor.is_none_or(|a| m.actor == a)
                        && own.is_none_or(|a| m.actor == a)
                        && f.object_id.is_none_or(|o| m.object == o)
                        && f.from.is_none_or(|t| m.at >= t)
                        && f.to.is_none_or(|t| m.at < t)
                })
                .map(|m| m.seq)
                .collect();
            let size = 1 + (i as i64 * 7) % 40;
            let (mut got, mut cursor, mut pages) = (vec![], None, 0);
            loop {
                let page = query(&mut tx, f, own, cursor, size).await?;
                assert!(page.items.len() as i64 <= size);
                got.extend(page.items.iter().filter(|e| e.action != "login.succeeded").map(|e| e.seq));
                pages += 1;
                match page.next_cursor {
                    Some(c) => cursor = explorer::parse_cursor(Some(&c))?,
                    None => break,
                }
                assert!(pages < 1000, "pagination terminates");
            }
            assert_eq!(got, expected, "filter #{i} own={own:?} page size {size}");
        }
    }
    // Names resolve for members only; diffs, IPs and hashes come through.
    let first = query(&mut tx, &Filter::default(), None, None, 1).await?.items.remove(0);
    assert_eq!((first.action.as_str(), first.actor_name.as_deref()), ("login.succeeded", None));
    let ada =
        query(&mut tx, &Filter { actor: Some(people[0].subject_ref), ..Filter::default() }, None, None, 1)
            .await?
            .items
            .remove(0);
    assert_eq!(ada.actor_name.as_deref(), Some("Ada"));
    assert_eq!(ada.hash.len(), 64);
    // Validation.
    assert!(matches!(
        query(&mut tx, &Filter { category: Some("bogus".into()), ..Filter::default() }, None, None, 10).await,
        Err(ExplorerError::Invalid(_))
    ));
    // Timelines: oldest first, only the requested objects, view-own respected.
    let tl = timeline(&mut tx, "thing", &[objects[1], objects[3]], None, None).await?;
    let want: Vec<i64> =
        model.iter().filter(|m| m.object == objects[1] || m.object == objects[3]).map(|m| m.seq).collect();
    assert_eq!(tl.items.iter().map(|e| e.seq).collect::<Vec<_>>(), want);
    let mine = timeline(&mut tx, "thing", &[objects[1]], Some(people[2].subject_ref), None).await?;
    assert!(mine.items.iter().all(|e| e.actor_subject_ref == Some(people[2].subject_ref)));
    assert!(timeline(&mut tx, "other", &[objects[1]], None, None).await?.items.is_empty());
    tx.rollback().await?;

    // Another workspace sees none of it.
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    assert!(query(&mut tx, &Filter::default(), None, None, 50).await?.items.is_empty());
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn saved_queries_are_private_and_audited_and_exports_are_audited() -> anyhow::Result<()> {
    let t = db_or_skip!();
    let clock = ManualClock::new(OffsetDateTime::now_utc());
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let mut tx = t.db.begin_ws(ws).await?;
    let (ada, ben) = (member(&mut tx, "Ada").await?, member(&mut tx, "Ben").await?);
    for i in 0..5 {
        let mut ev = AuditEvent::new(
            Some(ben.subject_ref),
            ActorKind::User,
            Category::Access,
            "member.invited",
            Outcome::Allowed,
        );
        ev.reason = Some(format!("=cmd|{i}, \"quoted\""));
        append(&mut tx, &clock, &UuidV7Gen, &ev).await?;
    }

    let f = Filter { action: Some("member.".into()), ..Filter::default() };
    let s = save_query(&mut tx, &clock, &UuidV7Gen, &ada, "Invites", &f).await?;
    assert_eq!((s.name.as_str(), &s.filters), ("Invites", &f));
    assert!(matches!(
        save_query(&mut tx, &clock, &UuidV7Gen, &ada, " invites ", &Filter::default()).await,
        Err(ExplorerError::Conflict(_))
    ));
    assert!(matches!(
        save_query(&mut tx, &clock, &UuidV7Gen, &ada, "", &Filter::default()).await,
        Err(ExplorerError::Invalid(_))
    ));
    save_query(&mut tx, &clock, &UuidV7Gen, &ben, "Invites", &Filter::default()).await?;
    assert_eq!(saved_queries(&mut tx, ada.membership).await?.len(), 1);
    assert!(matches!(
        delete_saved_query(&mut tx, &clock, &UuidV7Gen, &ben, s.id).await,
        Err(ExplorerError::NotFound)
    ));
    delete_saved_query(&mut tx, &clock, &UuidV7Gen, &ada, s.id).await?;
    assert!(saved_queries(&mut tx, ada.membership).await?.is_empty());
    let audit =
        query(&mut tx, &Filter { action: Some("audit.query_".into()), ..Filter::default() }, None, None, 10)
            .await?;
    assert_eq!(
        audit.items.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        vec!["audit.query_deleted", "audit.query_saved", "audit.query_saved"]
    );
    assert_eq!(audit.items[0].object_id, Some(s.id));

    // CSV: header + rows, formula-safe, truncated at the cap; the export itself is audited.
    let csv = export(&mut tx, &clock, &UuidV7Gen, &ada, &f, ExportFormat::Csv, 3).await?;
    assert_eq!((csv.rows, csv.truncated), (3, true));
    let lines: Vec<&str> = csv.body.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("seq,occurred_at,actor_subject_ref,actor_name"));
    assert!(lines[1].contains(",Ben,") && lines[1].contains("\"'=cmd|4, \"\"quoted\"\"\""), "{}", lines[1]);
    let jsonl = export(&mut tx, &clock, &UuidV7Gen, &ada, &f, ExportFormat::Jsonl, EXPORT_ROW_CAP).await?;
    assert_eq!((jsonl.rows, jsonl.truncated), (5, false));
    let first: serde_json::Value = serde_json::from_str(jsonl.body.lines().next().unwrap_or("{}"))?;
    assert_eq!(first["action"], "member.invited");
    let exports =
        query(&mut tx, &Filter { category: Some("export".into()), ..Filter::default() }, None, None, 10)
            .await?;
    assert_eq!(exports.items.len(), 2);
    assert_eq!(exports.items[0].actor_subject_ref, Some(ada.subject_ref));
    assert_eq!(exports.items[0].diff.as_ref().map(|d| d["rows"].clone()), Some(serde_json::json!(5)));
    assert_eq!(exports.items[1].diff.as_ref().map(|d| d["truncated"].clone()), Some(serde_json::json!(true)));
    tx.commit().await?;
    assert_eq!(verify(&t.db, ws).await?.first_broken, None);
    Ok(())
}
