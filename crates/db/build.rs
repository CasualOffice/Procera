// Re-embed migrations whenever a file in migrations/ changes (sqlx::migrate! reads them at compile time).
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
