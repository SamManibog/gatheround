use gatheround::raylib_integrations::RaylibEditor;

fn main() {
    RaylibEditor::run_standalone(
        std::env::current_dir().expect("Could not open current directory."),
    );
}
