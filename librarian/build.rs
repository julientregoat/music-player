fn main() {
    // TODO dynamically generate schemafile if not present at build time
    println!("cargo:rustc-env=DATABASE_URL=sqlite:schema.db");
}
