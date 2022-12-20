use std::time::Duration;

use egui::{
    vec2, Align, Button, CentralPanel, CursorIcon, Direction, FontFamily, FontId, Layout, Rect,
    Response, Sense, SidePanel, Slider, Stroke, Ui, Widget,
};
use rand::seq::IteratorRandom;

use crate::{
    game_state::{GameState, Player, PlayerState},
    mcts::MCTSContext,
};

pub struct MancalaApp {
    /// Whether UI debug mode is enabled.
    debug: bool,

    /// The history of game states.
    history: Vec<GameState>,

    /// The index of the active game state in `self.history`.
    active_state_index: usize,

    /// The current MCTS execution context.
    mcts_context: MCTSContext,
}

impl MancalaApp {
    /// Initializes an instance of the app.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        MancalaApp::set_styles(&cc.egui_ctx);
        Self {
            debug: false,
            history: vec![GameState::default()],
            active_state_index: 0,
            mcts_context: MCTSContext::new(Duration::from_secs_f64(1.0)),
        }
    }

    /// Sets up the app's styles and such.
    fn set_styles(ctx: &egui::Context) {
        use egui::TextStyle::*;

        // scale the whole UI
        ctx.set_pixels_per_point(1.5);

        // Get current context style
        let mut style = (*ctx.style()).clone();

        // Redefine text_styles
        style.text_styles = [
            (Small, FontId::new(9.0, FontFamily::Proportional)),
            (Body, FontId::new(12.5, FontFamily::Proportional)),
            (Monospace, FontId::new(12.0, FontFamily::Monospace)),
            (Button, FontId::new(12.5, FontFamily::Proportional)),
            (Heading, FontId::new(18.0, FontFamily::Proportional)),
        ]
        .into();

        // Mutate global style with above changes
        ctx.set_style(style);
    }

    /// Returns the active `GameState`.
    fn active_state(&mut self) -> &mut GameState {
        &mut self.history[self.active_state_index]
    }
}

impl eframe::App for MancalaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Self { debug, mcts_context, .. } = self;

        SidePanel::left("side_panel").show(ctx, |ui| {
            egui::warn_if_debug_build(ui);
            ui.heading("Side Panel");

            ui.checkbox(debug, "Debug");
            ctx.set_debug_on_hover(*debug);

            ui.label("MCTS think time (sec):");
            let mut seconds = mcts_context.choice_time_limit.as_secs_f64();
            let slider = Slider::new(&mut seconds, 0.0..=10.0).clamp_to_range(false);
            if ui.add(slider).changed() {
                mcts_context.choice_time_limit = Duration::from_secs_f64(seconds);
            }

            ui.label(format!("Node cache size:\n{}", mcts_context.cache_size()));
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Current Game State");

            let old_cur_player = self.active_state().cur_player;

            ui.add(self.active_state());

            if ui.button("MCTS move").clicked() {
                let game_state = self.active_state().clone();
                let move_to_make = self.mcts_context.mcts_choose(&game_state);
                if let Some(score) = self.active_state().make_move(move_to_make) {
                    println!("END: {score}");
                }
            }

            // if the current player has changed, reset animations to prevent briefly leaking
            // previous values from the last time this player was active
            if self.active_state().cur_player != old_cur_player {
                ui.ctx().clear_animations();
            }
        });
    }
}

impl Widget for &mut GameState {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut move_to_make = None;

        let res = ui
            .vertical_centered(|ui| {
                if self.result().is_some() {
                    ui.set_enabled(false);
                }

                ui.add_space(10.0);
                ui.spacing_mut().item_spacing.y = 10.0;

                ui.label("Player 2");
                ui.label(self.p2_state.store.to_string());

                ui.columns(2, |columns| {
                    let mut add_holes = |ui: &mut Ui, player_state: &PlayerState, on_left: bool| {
                        for (hole_index, &stones) in player_state.holes.iter().enumerate() {
                            if ui.add(hole(stones, on_left)).clicked() {
                                move_to_make = Some(hole_index);
                            }
                        }
                    };

                    columns[1].set_enabled(self.cur_player == Player::Player2);
                    columns[1].with_layout(Layout::top_down(Align::LEFT), |ui| {
                        add_holes(ui, &self.p2_state, false);
                    });

                    columns[0].set_enabled(self.cur_player == Player::Player1);
                    columns[0].set_height(columns[1].min_rect().height());
                    columns[0].with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                        add_holes(ui, &self.p1_state, true);
                    });
                });

                ui.label(self.p1_state.store.to_string());
                ui.label("Player 1");

                ui.add_space(0.0); // actually adds item_spacing
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

/// A widget that displays the button representing a hole on the game board.
pub fn hole_button(stones: u8) -> impl Widget {
    move |ui: &mut Ui| {
        let base_size = vec2(22.0, 20.0);
        let padding = vec2(4.0, 4.0);
        let button_size = base_size + padding;

        let button = Button::new(stones.to_string()).min_size(button_size);

        ui.add_enabled(stones > 0, button)
            .on_hover_cursor(CursorIcon::PointingHand)
    }
}

/// A widget that displays a bar indicating a quantity. Fills the available width.
pub fn value_bar(value: f32, max_value: f32, direction: Direction) -> impl Widget {
    move |ui: &mut Ui| {
        let width = ui.available_size_before_wrap().x;
        let height = ui.spacing().interact_size.y;
        let (outer_rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

        if ui.is_rect_visible(response.rect) {
            let visuals = ui.style().visuals.clone();
            let rounding = outer_rect.height() / 4.0;
            let proportion = (value / max_value).clamp(0.0, 1.0);
            let proportion = ui
                .ctx()
                .animate_value_with_time(response.id, proportion, 0.1);

            ui.painter()
                .rect(outer_rect, rounding, visuals.extreme_bg_color, Stroke::NONE);

            let inner_size = vec2(outer_rect.width() * proportion, outer_rect.height());
            let inner_rect = match direction {
                Direction::LeftToRight => Rect::from_min_size(outer_rect.min, inner_size),
                Direction::RightToLeft => {
                    Rect::from_min_max(outer_rect.max - inner_size, outer_rect.max)
                }
                direction => unimplemented!("value_bar with direction {direction:?}"),
            };
            ui.painter()
                .rect(inner_rect, rounding, visuals.selection.bg_fill, Stroke::NONE);
        }

        response
    }
}

/// A widget that displays a hole in the game board along with its extra information.
pub fn hole(stones: u8, on_left: bool) -> impl Widget {
    move |ui: &mut Ui| {
        let size = vec2(ui.available_width(), 22.0 + 4.0);
        let direction = if on_left {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };
        let layout = Layout::from_main_dir_and_cross_align(direction, Align::Center);
        ui.allocate_ui_with_layout(size, layout, |ui| {
            let button_response = ui.add(hole_button(stones));
            ui.add_visible(ui.is_enabled(), value_bar(stones as f32, 6.0, direction));
            button_response
        })
        .inner
    }
}
