fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("DOCS_RS").is_some() {
        // When building in docs.rs, we want to set SQLX_OFFLINE mode to true
        println!("cargo:rustc-env=SQLX_OFFLINE=true");
    }

    println!("cargo:rerun-if-changed=migrations");

    if std::env::var("PROTOC").ok().is_some() {
        println!("Using PROTOC set in environment.");
    } else {
        println!("Setting PROTOC to protoc-bin-vendored version.");
        std::env::set_var("PROTOC", protobuf_src::protoc());
    }

    tonic_build::configure()
        .extern_path(".google.protobuf.Struct", "::prost_wkt_types::Struct")
        .extern_path(".google.protobuf.Timestamp", "::prost_wkt_types::Timestamp")
        .compile_protos(&["proto/ledger/outbox_service.proto"], &["proto"])?;
    Ok(())
}
