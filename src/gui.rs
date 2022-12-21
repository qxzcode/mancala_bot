use egui::{
    vec2, Align, Button, CentralPanel, CursorIcon, Direction, FontFamily, FontId, Label, Layout,
    Rect, Sense, SidePanel, Slider, Stroke, Ui, Widget,
};
use itertools::Itertools;
use num_format::{Locale, ToFormattedString};
use num_traits::{Num, NumCast};
use rand::{seq::IteratorRandom, thread_rng};

use crate::{
    game_state::{GameState, Player, HOLES_PER_SIDE},
    mcts::{get_best_options, OptionStats, StateStats},
    worker::Worker,
};

pub struct MancalaApp {
    /// Whether UI debug mode is enabled.
    debug: bool,

    /// The history of game states.
    history: Vec<GameState>,

    /// The index of the active game state in `self.history`.
    active_state_index: usize,

    /// The manager for the worker thread.
    worker: Worker,
}

impl MancalaApp {
    /// Initializes an instance of the app.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        MancalaApp::set_styles(&cc.egui_ctx);

        let initial_game_state = GameState::default();
        let worker = Worker::spawn(&cc.egui_ctx);
        worker.set_active_state(initial_game_state.clone());

        Self {
            debug: false,
            history: vec![initial_game_state],
            active_state_index: 0,
            worker,
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
        let worker_data = self.worker.get_data();
        let node_cache_size = worker_data.as_ref().map(|data| data.node_cache_size);
        let state_stats = worker_data
            .filter(|data| &data.game_state == self.active_state())
            .map(|data| data.stats);

        SidePanel::left("side_panel").show(ctx, |ui| {
            egui::warn_if_debug_build(ui);
            ui.heading("Settings");

            ui.checkbox(&mut self.debug, "Debug");
            ctx.set_debug_on_hover(self.debug);

            ui.label("Node cache size limit:");
            let mut cache_size_limit = 4_000_000;
            let slider = Slider::new(&mut cache_size_limit, 1..=30_000_000).clamp_to_range(false);
            ui.add_enabled(false, slider);

            let size_string = node_cache_size
                .map_or_else(|| "...".into(), |n| n.to_formatted_string(&Locale::en));
            ui.label(format!("Node cache size:\n{}", size_string));
            ui.add(value_bar(
                node_cache_size.unwrap_or(0),
                cache_size_limit,
                Direction::LeftToRight,
            ));

            if ui.button("Clear cache").clicked() {
                self.worker.clear_cache();
            }
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.heading("Current Game State");

            let game_state = self.active_state();

            let mut game_state_changed =
                add_annotated_game_state(ui, game_state, state_stats.as_ref());

            let is_game_over = game_state.result().is_some();
            let single_valid_move = game_state.valid_moves().exactly_one().ok();
            let enable_mcts_button =
                !is_game_over && (state_stats.is_some() || single_valid_move.is_some());

            let button = Button::new("Best move (by MCTS)");
            if ui.add_enabled(enable_mcts_button, button).clicked() {
                let move_to_make = single_valid_move.unwrap_or_else(|| {
                    // pick a random best (maximum visit count) choice
                    let index = get_best_options(&state_stats.unwrap().options)
                        .choose(&mut thread_rng())
                        .unwrap();
                    game_state.valid_moves().nth(index).unwrap()
                });

                game_state.make_move(move_to_make);
                game_state_changed = true;
            }

            if game_state_changed {
                let active_state = game_state.clone();
                self.worker.set_active_state(active_state);
                ui.ctx().clear_animations();
            }
        });
    }
}

/// Adds a widget that displays the game state, annotated with extra information.
/// Returns whether the game state has changed.
pub fn add_annotated_game_state(
    ui: &mut Ui,
    game_state: &mut GameState,
    stats: Option<&StateStats>,
) -> bool {
    let mut move_to_make = None;

    // get the stats for each hole
    let mut hole_stats = [None; HOLES_PER_SIDE];
    if let Some(stats) = stats {
        for (hole_index, move_stats) in game_state.valid_moves().zip_eq(&stats.options) {
            hole_stats[hole_index] = Some(HoleStats {
                parent_rollouts: stats.num_rollouts,
                stats: move_stats,
            });
        }
    }
    let hole_stats = hole_stats; // make immutable

    ui.vertical_centered(|ui| {
        if game_state.result().is_some() {
            ui.set_enabled(false);
        }

        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 10.0;

        ui.label("Player 2");
        ui.label(game_state.p2_state.store.to_string());

        ui.columns(2, |columns| {
            let mut add_holes = |ui: &mut Ui, player: Player| {
                let on_left = player == Player::Player1;
                let player_state = game_state.player(player);
                let layout = if on_left {
                    Layout::bottom_up(Align::RIGHT)
                } else {
                    Layout::top_down(Align::LEFT)
                };
                let is_active_side = player == game_state.cur_player;

                ui.set_enabled(is_active_side);
                ui.with_layout(layout, |ui| {
                    for (hole_index, &stones) in player_state.holes.iter().enumerate() {
                        let stats = hole_stats[hole_index].filter(|_| is_active_side);
                        if ui.add(hole(stones, on_left, stats)).clicked() {
                            move_to_make = Some(hole_index);
                        }
                    }
                });
            };

            add_holes(&mut columns[1], Player::Player2);

            columns[0].set_height(columns[1].min_rect().height());
            add_holes(&mut columns[0], Player::Player1);
        });

        ui.label(game_state.p1_state.store.to_string());
        ui.label("Player 1");

        ui.add_space(0.0); // actually adds item_spacing
    });

    let is_game_over = game_state.result().is_some();
    if ui
        .add_enabled(!is_game_over, Button::new("Random move"))
        .clicked()
    {
        move_to_make = game_state
            .player(game_state.cur_player)
            .non_empty_holes()
            .choose(&mut rand::thread_rng());
    }

    if let Some(hole_index) = move_to_make {
        game_state.make_move(hole_index);
        return true;
    }

    false
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
pub fn value_bar<N>(value: N, max_value: N, direction: Direction) -> impl Widget
where
    N: Num + NumCast,
{
    move |ui: &mut Ui| {
        let width = ui.available_size_before_wrap().x;
        let height = ui.spacing().interact_size.y;
        let (outer_rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

        if ui.is_rect_visible(response.rect) {
            let visuals = ui.style().visuals.clone();
            let rounding = outer_rect.height() / 4.0;

            let value: f32 = num_traits::cast(value).unwrap();
            let max_value: f32 = num_traits::cast(max_value).unwrap();
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

#[derive(Clone, Copy)]
struct HoleStats<'a> {
    parent_rollouts: u32,
    stats: &'a OptionStats,
}

/// A widget that displays a hole in the game board along with its extra information.
fn hole(stones: u8, on_left: bool, stats: Option<HoleStats>) -> impl Widget + '_ {
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
            if let Some(stats) = stats {
                ui.add_visible_ui(ui.is_enabled(), |ui| {
                    ui.add_sized(
                        vec2(32.4, 14.0),
                        Label::new(format!("{:+.1}", stats.stats.expected_score())),
                    );
                    ui.add(value_bar(stats.stats.num_rollouts, stats.parent_rollouts, direction));
                });
            }
            button_response
        })
        .inner
    }
}
