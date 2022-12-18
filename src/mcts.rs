use itertools::Itertools;
use ordered_float::NotNan;
use rand::seq::IteratorRandom;
use rand::{thread_rng, Rng};

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::game_state::{GameState, Player};

/// Performs a randomized rollout from the given state and returns the final
/// score for Player 1.
#[must_use]
pub fn compute_rollout_score(mut game_state: GameState) -> i8 {
    let mut rng = thread_rng();

    loop {
        let random_move = game_state.valid_moves().choose(&mut rng).unwrap();
        if let Some(score) = game_state.make_move(random_move) {
            return score;
        }
    }
}

pub fn get_best_options(option_stats_arr: &[OptionStats]) -> impl Iterator<Item = usize> + '_ {
    let max_visit_count = option_stats_arr
        .iter()
        .map(|option_stats| option_stats.num_rollouts)
        .max()
        .expect("option_stats_vec is empty");

    option_stats_arr
        .iter()
        .enumerate()
        .filter(move |(_, option_stats)| option_stats.num_rollouts == max_visit_count)
        .map(|(option_index, _)| option_index)
}

#[derive(Debug, Clone)]
pub struct OptionStats {
    pub num_rollouts: u32,
    pub total_score: i64,
}

impl OptionStats {
    /// Returns the estimated expected score for this option.
    #[must_use]
    pub fn expected_score(&self) -> NotNan<f64> {
        if self.num_rollouts == 0 {
            NotNan::new(0.0).unwrap()
        } else {
            let expected_score = (self.total_score as f64) / ((self.num_rollouts * 2) as f64);
            NotNan::new(expected_score).expect("expected score is NaN")
        }
    }

    /// The UCB1 score for a choice.
    /// https://gibberblot.github.io/rl-notes/single-agent/multi-armed-bandits.html
    #[must_use]
    pub fn ucb1_score(&self, rollout_num: usize) -> NotNan<f64> {
        self.expected_score()
            + (2.0 * (rollout_num as f64).ln() / (self.num_rollouts as f64)).sqrt()
    }

    /// A variant of the PUCT score, similar to that used in AlphaZero.
    #[must_use]
    pub fn puct_score(&self, parent_rollouts: u32) -> NotNan<f64> {
        let exploration_rate = 1.0; // TODO: make this a tunable parameter
        let exploration_score =
            exploration_rate * (parent_rollouts as f64).sqrt() / ((1 + self.num_rollouts) as f64);
        self.expected_score() + exploration_score
    }
}

#[derive(Debug, Clone)]
struct StateStats {
    options: Box<[OptionStats]>,
    num_rollouts: u32,
    last_visit_ply: u32,
}

impl StateStats {
    #[must_use]
    fn new(num_options: usize, current_ply: u32) -> Self {
        debug_assert!(num_options > 1, "Expanded a state with less than 2 options");
        Self {
            options: vec![
                OptionStats {
                    num_rollouts: 0,
                    total_score: 0,
                };
                num_options
            ]
            .into(),
            num_rollouts: 0,
            last_visit_ply: current_ply,
        }
    }
}

pub struct MCTSContext {
    pub choice_time_limit: Duration,

    explored_states: HashMap<GameState, StateStats>,
    current_ply: u32,
}

impl MCTSContext {
    #[must_use]
    pub fn new(choice_time_limit: Duration) -> Self {
        Self {
            choice_time_limit,
            explored_states: HashMap::new(),
            current_ply: 0,
        }
    }

    /// Returns the number of explored nodes currently in the cache.
    #[must_use]
    pub fn cache_size(&self) -> usize {
        self.explored_states.len()
    }

    #[must_use]
    fn get_root_option_stats(&self, game_state: &GameState) -> (u32, &[OptionStats]) {
        self.explored_states
            .get(game_state)
            .map(|stats| (stats.num_rollouts, stats.options.as_ref()))
            .expect("root state not explored")
    }

    fn prune_explored_states(&mut self) {
        const PAST_PLIES_TO_KEEP: u32 = 5;
        if self.current_ply > PAST_PLIES_TO_KEEP {
            let cutoff_ply = self.current_ply - PAST_PLIES_TO_KEEP;
            self.explored_states
                .retain(|_, state_stats| state_stats.last_visit_ply >= cutoff_ply);
        }
    }

    /// Runs MCTS to choose an option.
    pub fn mcts_choose(&mut self, game_state: &GameState) -> usize {
        // return immediately without searching if there's only one option
        if let Ok(only_option) = game_state.valid_moves().exactly_one() {
            return only_option;
        }

        let start_time = Instant::now();

        self.current_ply += 1;
        self.prune_explored_states();

        let mut last_print_time = start_time;
        let mut num_samples = 0;
        while start_time.elapsed() < self.choice_time_limit {
            // sample a sequence of moves and update the tree
            self.sample_move(game_state.clone());
            num_samples += 1;

            // update the live stats display
            let now = Instant::now();
            let elapsed = now.duration_since(last_print_time);
            if elapsed > Duration::from_millis(100) {
                // TODO: show live stats in the GUI
                println!(
                    "Thinking, num_samples={num_samples}: [{}]",
                    self.get_root_option_stats(game_state)
                        .1
                        .iter()
                        .map(|x| format!("{:.2}", x.expected_score().into_inner()))
                        .join(", ")
                );
                last_print_time = now;
            }
        }

        // return a random best (maximum visit count) choice
        let index = get_best_options(self.get_root_option_stats(game_state).1)
            .choose(&mut thread_rng())
            .unwrap();
        game_state.valid_moves().nth(index).unwrap()
    }

    /// Samples a move that a player might make from a state, updating the search tree.
    /// Returns the rollout score for Player 1.
    fn sample_move(&mut self, game_state: GameState) -> i8 {
        let valid_moves = game_state.valid_moves().collect_vec();
        let num_options = valid_moves.len();

        // immediately continue to the next move if there's only one option
        if num_options == 1 {
            let mut game_state = game_state;
            let score = match game_state.make_move(valid_moves[0]) {
                Some(score) => score,
                None => self.sample_move(game_state),
            };
            return score;
        }

        // get which player needs to make a move
        let chooser = game_state.cur_player;

        let update_state_stats =
            |state_stats: &mut StateStats, option_index: usize, rollout_score: i8| {
                state_stats.num_rollouts += 1;
                let option_stats = &mut state_stats.options[option_index];
                option_stats.num_rollouts += 1;
                option_stats.total_score += i64::from(match chooser {
                    Player::Player1 => rollout_score,
                    Player::Player2 => -rollout_score,
                });
            };

        // sample an option and the score for Player 1
        match self.explored_states.entry(game_state.clone()) {
            Entry::Vacant(entry) => {
                // this is the first time we've seen this state, so create a new entry
                let state_stats = entry.insert(StateStats::new(num_options, self.current_ply));

                // at leaf nodes, start by sampling a random option
                let option_index = thread_rng().gen_range(0..num_options);
                let next_move = valid_moves[option_index];

                // perform a rollout from this state
                let mut game_state = game_state;
                let score = match game_state.make_move(next_move) {
                    Some(score) => score,
                    None => compute_rollout_score(game_state),
                };

                // update the stats for this option
                update_state_stats(state_stats, option_index, score);

                score
            }
            Entry::Occupied(entry) => {
                // this state has been seen before; get the stored stats
                let state_stats = entry.into_mut();
                state_stats.last_visit_ply = self.current_ply;

                // choose an option based on the current stats
                let (option_index, (_, next_move)) = state_stats
                    .options
                    .iter()
                    .zip_eq(valid_moves)
                    .enumerate()
                    .max_by_key(|(_, (option_stats, _))| {
                        option_stats.puct_score(state_stats.num_rollouts)
                    })
                    .unwrap();

                // get the next state and recurse (or return the result if the game ended)
                let mut game_state2 = game_state.clone();
                let score = match game_state2.make_move(next_move) {
                    Some(score) => score,
                    None => self.sample_move(game_state2),
                };

                // update the stats for this option
                let state_stats = self.explored_states.get_mut(&game_state).unwrap();
                update_state_stats(state_stats, option_index, score);

                score
            }
        }
    }
}
