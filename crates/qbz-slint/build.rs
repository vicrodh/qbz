//! Compiles the Slint UI tree with bundled translations. `ui/app.slint` is the
//! single entry point. Translations are bundled (pure-Rust, no C dep) from
//! `translations/<lang>/LC_MESSAGES/qbz-slint.po`; msgid = English source, no context.
fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("Slint UI failed to compile");
}
