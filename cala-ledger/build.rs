fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("DOCS_RS").is_some() {
        // When building in docs.rs, we want to set SQLX_OFFLINE mode to true
        println!("cargo:rustc-env=SQLX_OFFLINE=true");
    }

    println!("cargo:rerun-if-changed=migrations");

    std::env::set_var("PROTOC", protobuf_src::protoc());
    tonic_build::configure()
        .extern_path(".google.protobuf.Struct", "::prost_wkt_types::Struct")
        .extern_path(".google.protobuf.Timestamp", "::prost_wkt_types::Timestamp")
        .compile(&["proto/ledger/outbox_service.proto"], &["proto"])?;
    Ok(())
}
