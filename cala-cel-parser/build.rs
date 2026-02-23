fn main() {
    #[cfg(feature = "regenerate-parser")]
    lalrpop::process_root().unwrap();
}
