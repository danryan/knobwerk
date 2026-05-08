use slint_build::{CompilerConfiguration, EmbedResourcesKind};

fn main() {
    let config = CompilerConfiguration::new()
        .embed_resources(EmbedResourcesKind::EmbedForSoftwareRenderer);

    slint_build::compile_with_config("ui/appwindow.slint", config)
        .expect("Slint compile failed");
}
