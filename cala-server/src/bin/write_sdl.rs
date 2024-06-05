fn main() {
    println!(
        "{}",
        cala_server::graphql::schema::<
            cala_server::extension::core::QueryExtension,
            cala_server::extension::core::MutationExtension,
        >(None)
        .sdl()
        .trim()
    );
}
