use egui::vec2;

pub mod game_state;
pub mod gui;
pub mod mcts;
pub mod worker;

fn main() {
    let native_options = eframe::NativeOptions {
        min_window_size: Some(vec2(300.0, 200.0)),
        initial_window_size: Some(vec2(1000.0, 650.0)),
        ..Default::default()
    };
    eframe::run_native(
        "MancalaBot",
        native_options,
        Box::new(|cc| Box::new(gui::MancalaApp::new(cc))),
    );
}
