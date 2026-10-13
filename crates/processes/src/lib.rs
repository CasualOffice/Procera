//! Published processes (BPMS-084, ADR-0012). A process is a library artifact whose content is its
//! IR document. When a version is published, [`PublishProcesses`] compiles it against a snapshot of
//! the workspace (every `kind:key@version` it references must be published; every named group,
//! role, position or person must exist) and writes an immutable release bundle: the exact artifact
//! versions it pinned, a hash of that manifest and the compiled program's checksum. A compile error
//! refuses the publish. [`DbPrograms`] then serves those programs to the engine by key, version and
//! checksum, recompiling once per checksum and refusing a program whose checksum no longer matches
//! its bundle; [`Chain`] puts it behind the built-in programs.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use procera_db::{Db, WsCtx, WsTx};
use procera_engine::{BoxFuture, Executable, ProgramRef, ProgramSource};
use procera_ir::model::{CandidateType, IrDocument};
use procera_ir::{CompileContext, compile};
use procera_ir::{Environment, Resolution};
use procera_kernel::{Clock, IdGen};
use procera_library::{PublishHook, Publishing};
use procera_runtime::Program;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// What a reference resolved to in the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pinned {
    artifact_id: Uuid,
    version_id: Uuid,
}

/// The workspace as the compiler may see it, loaded before compiling (no I/O inside the compiler).
#[derive(Debug, Default)]
pub struct Snapshot {
    refs: BTreeMap<String, (Resolution, Option<Pinned>)>,
    groups: BTreeSet<String>,
    roles: BTreeSet<String>,
    positions: BTreeSet<String>,
    users: BTreeSet<String>,
}

/// `kind:key@version` → its parts.
fn parse_ref(r: &str) -> Option<(&str, &str, i32)> {
    let (kind, rest) = r.split_once(':')?;
    let (key, version) = rest.rsplit_once('@')?;
    Some((kind, key, version.parse().ok()?))
}

impl Snapshot {
    /// Resolves every reference `doc` makes and loads the directory names it may use.
    pub async fn load(tx: &mut WsTx<'_>, doc: &IrDocument) -> Result<Self, sqlx::Error> {
        let mut s = Self::default();
        for (_, reference) in doc.refs.entries() {
            let Some((kind, key, no)) = parse_ref(reference) else {
                s.refs.insert(reference.to_owned(), (Resolution::NotFound, None));
                continue;
            };
            let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
                "SELECT a.id, v.id, v.lifecycle FROM artifacts a JOIN artifact_versions v ON v.workspace_id = a.workspace_id AND v.artifact_id = a.id
                  WHERE a.kind = $1 AND a.key = $2 AND v.version_no = $3",
            )
            .bind(kind)
            .bind(key)
            .bind(no)
            .fetch_optional(tx.conn())
            .await?;
            let entry = match row {
                None => (Resolution::NotFound, None),
                Some((a, v, lifecycle)) if lifecycle == "published" => {
                    (Resolution::Published, Some(Pinned { artifact_id: a, version_id: v }))
                }
                Some(_) => (Resolution::NotPublished, None),
            };
            s.refs.insert(reference.to_owned(), entry);
        }
        let names = |sql: &'static str| sqlx::query_scalar::<_, String>(sql);
        s.groups = names("SELECT name FROM groups").fetch_all(tx.conn()).await?.into_iter().collect();
        s.roles = names("SELECT key FROM roles").fetch_all(tx.conn()).await?.into_iter().collect();
        s.positions = names("SELECT title FROM positions").fetch_all(tx.conn()).await?.into_iter().collect();
        s.users = names(
            "SELECT lower(u.email::text) FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.status = 'active'
             UNION SELECT m.id::text FROM memberships m WHERE m.status = 'active'",
        )
        .fetch_all(tx.conn())
        .await?
        .into_iter()
        .collect();
        Ok(s)
    }

    /// The pinned versions, as the bundle manifest records them.
    fn manifest(&self) -> Value {
        let pins: serde_json::Map<String, Value> = self
            .refs
            .iter()
            .filter_map(|(r, (_, p))| {
                p.map(|p| (r.clone(), json!({ "artifact_id": p.artifact_id, "version_id": p.version_id })))
            })
            .collect();
        Value::Object(pins)
    }
}

impl Environment for Snapshot {
    fn resolve(&self, reference: &str) -> Resolution {
        self.refs.get(reference).map_or(Resolution::NotFound, |e| e.0)
    }
    fn connector_usable(&self, _reference: &str) -> bool {
        false // connectors arrive with BPMS-120; until then none are usable.
    }
    fn principal_exists(&self, ty: CandidateType, name: &str) -> bool {
        match ty {
            CandidateType::Group => self.groups.contains(name),
            CandidateType::Role => self.roles.contains(name),
            CandidateType::Position => self.positions.contains(name),
            CandidateType::User => self.users.contains(&name.to_lowercase()),
            CandidateType::DirectoryRule => true,
        }
    }
}

/// Compiles a published process and writes its release bundle (the library's publish hook).
#[derive(Debug)]
pub struct PublishProcesses {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl PublishProcesses {
    pub fn new(clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { clock, ids }
    }

    async fn bundle(&self, tx: &mut WsTx<'_>, v: &Publishing) -> Result<(), String> {
        let content: Value = sqlx::query_scalar("SELECT content FROM artifact_versions WHERE id = $1")
            .bind(v.version_id)
            .fetch_one(tx.conn())
            .await
            .map_err(|e| e.to_string())?;
        let doc: IrDocument = serde_json::from_value(content.clone())
            .map_err(|e| format!("this process is not a valid definition: {e}"))?;
        if doc.process.key != v.key {
            return Err(format!(
                "the definition names process '{}', but this artifact is '{}'",
                doc.process.key, v.key
            ));
        }
        if i64::from(doc.process.version) != i64::from(v.version_no) {
            return Err(format!(
                "the definition says version {}, but this is version {}",
                doc.process.version, v.version_no
            ));
        }
        let snapshot = Snapshot::load(tx, &doc).await.map_err(|e| e.to_string())?;
        let out = compile(&content, &CompileContext { environment: Some(&snapshot), previous: None });
        let Some(compiled) = out.compiled else {
            let errors: Vec<String> = out
                .diagnostics
                .iter()
                .filter(|d| d.severity == procera_ir::Severity::Error)
                .map(|d| format!("{} at {}: {}", d.code.as_str(), d.pointer, d.message))
                .collect();
            return Err(format!("this process can't be published: {}", errors.join("; ")));
        };
        let program = Program::from_compiled(&compiled).map_err(|e| e.to_string())?;
        let checksum = program.checksum().ok_or("the compiled program has no checksum")?.to_owned();
        let manifest = snapshot.manifest();
        let manifest_hash =
            Sha256::digest(serde_json::to_vec(&manifest).map_err(|e| e.to_string())?).to_vec();
        sqlx::query(
            "INSERT INTO release_bundles (workspace_id, id, artifact_id, process_version_id, process_key, version_no, manifest, manifest_hash, program_checksum, created_at)
             VALUES (current_setting('app.workspace_id')::uuid, $1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(self.ids.new_id())
        .bind(v.artifact_id)
        .bind(v.version_id)
        .bind(&v.key)
        .bind(v.version_no)
        .bind(&manifest)
        .bind(&manifest_hash)
        .bind(&checksum)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl PublishHook for PublishProcesses {
    fn published<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        version: &'a Publishing,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if version.kind != "process" {
                return Ok(());
            }
            self.bundle(tx, version).await
        })
    }
}

/// Published programs from release bundles, compiled once per checksum.
#[derive(Debug)]
pub struct DbPrograms {
    db: Db,
    cache: Mutex<HashMap<String, Arc<Executable>>>,
}

impl DbPrograms {
    pub fn new(db: Db) -> Self {
        Self { db, cache: Mutex::new(HashMap::new()) }
    }

    async fn find(&self, ctx: WsCtx, p: &ProgramRef) -> Result<Arc<Executable>, String> {
        let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
        let row: Option<(String, Value)> = sqlx::query_as(
            "SELECT b.program_checksum, v.content FROM release_bundles b
               JOIN artifact_versions v ON v.workspace_id = b.workspace_id AND v.id = b.process_version_id
              WHERE b.process_key = $1 AND b.version_no = $2 AND ($3::text IS NULL OR b.program_checksum = $3)
              ORDER BY b.created_at DESC LIMIT 1",
        )
        .bind(&p.process_key)
        .bind(i32::try_from(p.version).unwrap_or(i32::MAX))
        .bind(&p.checksum)
        .fetch_optional(tx.conn())
        .await
        .map_err(|e| e.to_string())?;
        tx.rollback().await.map_err(|e| e.to_string())?;
        let (checksum, content) =
            row.ok_or_else(|| format!("no published program {}@{}", p.process_key, p.version))?;
        if let Some(hit) = self.cache.lock().map_err(|e| e.to_string())?.get(&checksum) {
            return Ok(hit.clone());
        }
        let out = compile(&content, &CompileContext::default());
        let compiled =
            out.compiled.ok_or_else(|| format!("{}@{} no longer compiles", p.process_key, p.version))?;
        let program = Program::from_compiled(&compiled).map_err(|e| e.to_string())?;
        if program.checksum() != Some(checksum.as_str()) {
            return Err(format!(
                "{}@{}: the program does not match its release bundle",
                p.process_key, p.version
            ));
        }
        let exe = Arc::new(Executable::new(program));
        self.cache.lock().map_err(|e| e.to_string())?.insert(checksum, exe.clone());
        Ok(exe)
    }
}

impl ProgramSource for DbPrograms {
    fn load<'a>(
        &'a self,
        ctx: WsCtx,
        program: &'a ProgramRef,
    ) -> BoxFuture<'a, Result<Arc<Executable>, String>> {
        Box::pin(self.find(ctx, program))
    }
}

/// Tries each source in order (the built-ins first, then published processes).
#[derive(Debug)]
pub struct Chain(pub Vec<Arc<dyn ProgramSource>>);

impl ProgramSource for Chain {
    fn load<'a>(
        &'a self,
        ctx: WsCtx,
        program: &'a ProgramRef,
    ) -> BoxFuture<'a, Result<Arc<Executable>, String>> {
        Box::pin(async move {
            let mut last = String::from("no program sources");
            for s in &self.0 {
                match s.load(ctx, program).await {
                    Ok(e) => return Ok(e),
                    Err(e) => last = e,
                }
            }
            Err(last)
        })
    }
}
