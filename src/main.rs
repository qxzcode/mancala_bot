pub mod game_state;
pub mod gui;
pub mod mcts;
pub mod worker;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "MancalaBot",
        native_options,
        Box::new(|cc| Box::new(gui::MancalaApp::new(cc))),
    );
}
