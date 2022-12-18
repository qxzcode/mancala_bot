use std::time::Duration;

use rand::seq::IteratorRandom;

use crate::{
    game_state::{GameState, Player, PlayerState},
    mcts::MCTSContext,
};

pub struct MancalaApp {
    debug: bool,
    game_state: GameState,
    mcts_context: MCTSContext,
}

impl MancalaApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            debug: false,
            game_state: GameState::default(),
            mcts_context: MCTSContext::new(Duration::from_secs_f64(1.0)),
        }
    }
}

impl eframe::App for MancalaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Self {
            debug,
            mcts_context,
            ..
        } = self;

        egui::SidePanel::left("side_panel").show(ctx, |ui| {
            egui::warn_if_debug_build(ui);
            ui.heading("Side Panel");

            ui.checkbox(debug, "Debug");
            ctx.set_debug_on_hover(*debug);

            ui.label("MCTS think time:");
            let mut seconds = mcts_context.choice_time_limit.as_secs_f64();
            let slider = egui::Slider::new(&mut seconds, 0.0..=10.0);
            if ui.add(slider).changed() {
                mcts_context.choice_time_limit = Duration::from_secs_f64(seconds);
            }

            ui.label(format!("Node cache size:\n{}", mcts_context.cache_size()));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Current Game State");
            ui.add(&mut self.game_state);

            if ui.button("MCTS move").clicked() {
                let move_to_make = mcts_context.mcts_choose(&self.game_state);
                if let Some(score) = self.game_state.make_move(move_to_make) {
                    println!("END: {score}");
                }
            }
        });
    }
}

impl egui::Widget for &mut GameState {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut move_to_make = None;

        let res = ui
            .vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 10.0;

                ui.label("Player 2");
                ui.label(self.p2_state.store.to_string());

                ui.columns(2, |columns| {
                    let mut add_holes = |ui: &mut egui::Ui, player_state: &PlayerState| {
                        for (hole_index, &stones) in player_state.holes.iter().enumerate() {
                            if ui
                                .add_enabled(stones > 0, egui::Button::new(stones.to_string()))
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                move_to_make = Some(hole_index);
                            }
                        }
                    };

                    columns[1].set_enabled(self.cur_player == Player::Player2);
                    columns[1].with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        add_holes(ui, &self.p2_state);
                    });

                    columns[0].set_enabled(self.cur_player == Player::Player1);
                    columns[0].set_height(columns[1].min_rect().height());
                    columns[0].with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                        add_holes(ui, &self.p1_state);
                    });
                });

                ui.label(self.p1_state.store.to_string());
                ui.label("Player 1");
            })
            .response;

        if ui.button("Random move").clicked() {
            move_to_make = self
                .player(self.cur_player)
                .non_empty_holes()
                .choose(&mut rand::thread_rng());
        }

        if let Some(hole_index) = move_to_make {
            println!("{self:?}");
            if let Some(score) = self.make_move(hole_index) {
                println!("END: {score}");
            }
        }

        res
    }
}
