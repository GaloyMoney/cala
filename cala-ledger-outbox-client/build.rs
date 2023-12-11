fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=migrations");
    std::env::set_var("PROTOC", protobuf_src::protoc());

    tonic_build::configure()
        .extern_path(".google.protobuf.Struct", "::prost_wkt_types::Struct")
        .extern_path(".google.protobuf.Timestamp", "::prost_wkt_types::Timestamp")
        .compile(&["../proto/ledger/outbox_service.proto"], &["../proto"])?;
    Ok(())
}
