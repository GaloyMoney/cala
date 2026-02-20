fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("DOCS_RS").is_some() {
        println!("cargo:rustc-env=SQLX_OFFLINE=true");
    }
    Ok(())
}
