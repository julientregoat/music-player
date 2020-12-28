fn main() {
    // TODO dynamically generate schemafile if not present at build time
    // using sqlx migrator maybe?
    println!("cargo:rustc-env=DATABASE_URL=sqlite:schema.db");
}
