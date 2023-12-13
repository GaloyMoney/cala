fn main() {
    println!("{}", cala_server::graphql::schema(None).sdl().trim());
}
