use async_graphql::SDLExportOptions;

fn main() {
    println!(
        "{}",
        cala_server::graphql::schema(None)
            .sdl_with_options(SDLExportOptions::new().federation())
            .trim()
    );
}
