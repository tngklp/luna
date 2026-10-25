//! Render the TUI to text so the layout can be inspected without a terminal.
//!
//! Run with: `cargo run -p luna-tui --example preview`

use luna_core::config::Config;
use luna_core::provider::ProviderRegistry;
use luna_core::session::SessionStore;
use luna_tui::app::{App, Line};
use luna_tui::commands::Palette;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn main() {
    let dir = std::env::temp_dir().join("luna-preview");
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config::default();
    let providers = std::sync::Arc::new(ProviderRegistry::new(&config));
    let store = SessionStore::open(&dir).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let mut app = App::new(
        config,
        providers,
        store,
        std::sync::Arc::new(luna_core::event::NullSink),
        runtime.handle().clone(),
        "/work/.luna/config.toml".to_string(),
        true,
    )
    .unwrap();

    let mut term = Terminal::new(TestBackend::new(96, 22)).unwrap();
    term.draw(|f| luna_tui::ui::draw(f, &app)).unwrap();
    println!("=== empty state ===");
    print_buffer(&term);

    app.lines.push(Line::User("fix the tokenizer".into()));
    app.lines.push(Line::Assistant(
        "Found it. The tokenizer drops trailing whitespace.\n\n\
         ```rust\nfn tokenize(s: &str) -> Vec<Token> {\n    s.split_whitespace().map(Token::from).collect()\n}\n```\n\n\
         Then I ran the tests."
            .into(),
    ));
    app.lines.push(Line::Tool {
        name: "bash".into(),
        detail: "cargo test -p parser".into(),
        running: false,
        is_error: false,
    });
    app.lines.push(Line::ToolOutput("test result: ok. 14 passed".into()));
    term.draw(|f| luna_tui::ui::draw(f, &app)).unwrap();
    println!("\n=== conversation ===");
    print_buffer(&term);

    app.input = "/".into();
    app.palette = Palette::from_input("/");
    term.draw(|f| luna_tui::ui::draw(f, &app)).unwrap();
    println!("\n=== palette ===");
    print_buffer(&term);
}

fn print_buffer(term: &Terminal<TestBackend>) {
    let buf = term.backend().buffer().clone();
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        println!("{}", line.trim_end());
    }
}
