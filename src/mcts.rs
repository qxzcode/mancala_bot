use ahash::AHashMap;
use arrayvec::ArrayVec;
use itertools::Itertools;
use ordered_float::NotNan;
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};

use std::collections::hash_map::Entry;
use std::iter;
use std::time::{Duration, Instant};

use crate::game_state::{GameState, Player, HOLES_PER_SIDE};

/// Performs a randomized rollout from the given state and returns the final
/// score for Player 1.
#[must_use]
pub fn compute_rollout_score(mut game_state: GameState) -> i8 {
    let mut rng = thread_rng();

    loop {
        if let Some(score) = game_state.result() {
            return score;
        }

        let valid_moves = game_state
            .valid_moves()
            .collect::<ArrayVec<_, HOLES_PER_SIDE>>();
        let random_move = *valid_moves
            .choose(&mut rng)
            .expect("GameState should have at least one valid move");
        game_state.make_move(random_move);
    }
}

pub fn get_best_options(option_stats_arr: &[OptionStats]) -> impl Iterator<Item = usize> + '_ {
    let max_visit_count = option_stats_arr
        .iter()
        .map(|option_stats| option_stats.num_rollouts)
        .max()
        .expect("option_stats_arr is empty");

    option_stats_arr
        .iter()
        .enumerate()
        .filter(move |(_, option_stats)| option_stats.num_rollouts == max_visit_count)
        .map(|(option_index, _)| option_index)
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(packed)]
pub struct OptionStats {
    pub num_rollouts: u32,
    pub total_score: i64,
}

impl OptionStats {
    /// Returns the estimated expected score for this option.
    #[must_use]
    pub fn expected_score(&self) -> NotNan<f32> {
        if self.num_rollouts == 0 {
            NotNan::new(0.0).unwrap()
        } else {
            let expected_score = (self.total_score as f32) / (self.num_rollouts as f32);
            NotNan::new(expected_score).expect("expected score is NaN")
        }
    }

    /// The UCB1 score for a choice.
    /// https://gibberblot.github.io/rl-notes/single-agent/multi-armed-bandits.html
    #[must_use]
    pub fn ucb1_score(&self, rollout_num: usize) -> NotNan<f32> {
        self.expected_score()
            + (2.0 * (rollout_num as f32).ln() / (self.num_rollouts as f32)).sqrt()
    }

    /// A variant of the PUCT score, similar to that used in AlphaZero.
    #[must_use]
    pub fn puct_score(&self, parent_rollouts: u32) -> NotNan<f32> {
        let exploration_rate = 100.0; // TODO: make this a tunable parameter
        let exploration_score =
            exploration_rate * (parent_rollouts as f32).sqrt() / ((1 + self.num_rollouts) as f32);
        self.expected_score() + exploration_score
    }
}

#[derive(Debug, Clone)]
pub struct StateStats {
    pub options: ArrayVec<OptionStats, HOLES_PER_SIDE>,
    pub num_rollouts: u32,
    last_visit_ply: u32,
}

impl StateStats {
    #[must_use]
    fn new(num_options: usize, current_ply: u32) -> Self {
        debug_assert!(num_options > 1, "Expanded a state with less than 2 options");
        Self {
            options: iter::repeat_with(OptionStats::default)
                .take(num_options)
                .collect(),
            num_rollouts: 0,
            last_visit_ply: current_ply,
        }
    }
}

pub struct MCTSContext {
    explored_states: AHashMap<GameState, StateStats>,
    current_ply: u32,

    /// The (approximate) limit on the number of nodes to retain in the cache.
    pub cache_size_limit: usize,
}

impl MCTSContext {
    #[must_use]
    pub fn new(cache_size_limit: usize) -> Self {
        Self {
            explored_states: AHashMap::new(),
            current_ply: 0,
            cache_size_limit,
        }
    }

    /// Returns the number of explored nodes currently in the cache.
    #[must_use]
    pub fn cache_size(&self) -> usize {
        self.explored_states.len()
    }

    /// Clears the explored node cache.
    pub fn clear_cache(&mut self) {
        self.explored_states = AHashMap::new();
        self.current_ply = 0;
    }

    fn prune_explored_states(&mut self) {
        if self.cache_size() > self.cache_size_limit {
            let mut last_visit_plies = self
                .explored_states
                .values()
                .map(|state_stats| state_stats.last_visit_ply)
                .collect_vec();
            let index = last_visit_plies.len() / 7; // drop the stalest ~14.3%
            let (_, &mut cutoff_ply, _) = last_visit_plies.select_nth_unstable(index);

            self.explored_states
                .retain(|_, state_stats| state_stats.last_visit_ply >= cutoff_ply);
        }
    }

    /// Performs MCTS iterations on the given game state for the given amount of time.
    /// Returns the number of iterations/samples performed.
    pub fn ponder(&mut self, game_state: &GameState, duration: Duration) -> usize {
        let start_time = Instant::now();

        self.current_ply += 1;
        self.prune_explored_states();

        let mut num_samples = 0;
        while start_time.elapsed() < duration {
            // sample a sequence of moves and update the tree
            self.sample_move(game_state.clone());
            num_samples += 1;
        }
        num_samples
    }

    /// Returns the cached `StateStats` for a given game state.
    #[must_use]
    pub fn stats_for(&self, game_state: &GameState) -> Option<&StateStats> {
        self.explored_states.get(game_state)
    }

    /// Samples a move that a player might make from a state, updating the search tree.
    /// Returns the rollout score for Player 1.
    fn sample_move(&mut self, game_state: GameState) -> i8 {
        // return the game result if this is a terminal state
        if let Some(score) = game_state.result() {
            return score;
        }

        let valid_moves = game_state
            .valid_moves()
            .collect::<ArrayVec<_, HOLES_PER_SIDE>>();
        let num_options = valid_moves.len();

        // if there's only one option, immediately continue to the next move (without consulting or
        // updating the search tree)
        if num_options == 1 {
            let mut game_state = game_state;
            game_state.make_move(valid_moves[0]);
            return self.sample_move(game_state);
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
                game_state.make_move(next_move);
                let score = compute_rollout_score(game_state);

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
                game_state2.make_move(next_move);
                let score = self.sample_move(game_state2);

                // update the stats for this option
                let state_stats = self.explored_states.get_mut(&game_state).unwrap();
                update_state_stats(state_stats, option_index, score);

                score
            }
        }
    }
}
