fn main() {
    println!(
        "{}",
        cala_server::graphql::schema::<cala_server::extensions::AdditionalMutations>(None)
            .sdl()
            .trim()
    );
}
