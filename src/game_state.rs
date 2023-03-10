use std::{fmt, mem};

use static_assertions::const_assert;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Player {
    Player1,
    Player2,
}

impl Player {
    /// Returns the other player.
    #[must_use]
    pub fn other(&self) -> Player {
        match self {
            Player::Player1 => Player::Player2,
            Player::Player2 => Player::Player1,
        }
    }
}

impl fmt::Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Player::Player1 => "Player 1",
            Player::Player2 => "Player 2",
        })
    }
}

/// The number of holes on each player's side, not including their store.
pub const HOLES_PER_SIDE: usize = 6;

/// The number of initial stones in each hole.
pub const INITIAL_STONES_PER_HOLE: u8 = 4;

// Assert that the total number of stones in the game will fit in an i8.
const_assert!(HOLES_PER_SIDE * 2 * (INITIAL_STONES_PER_HOLE as usize) <= (i8::MAX as usize));

/// Represents a game state.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct GameState {
    /// Which player's turn it currently is.
    pub cur_player: Player,

    /// Player 1's state.
    pub p1_state: PlayerState,

    /// Player 2's state.
    pub p2_state: PlayerState,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            cur_player: Player::Player1,
            p1_state: PlayerState::default(),
            p2_state: PlayerState::default(),
        }
    }
}

impl GameState {
    /// Returns a reference to the state for the given player.
    #[must_use]
    pub fn player(&self, player: Player) -> &PlayerState {
        match player {
            Player::Player1 => &self.p1_state,
            Player::Player2 => &self.p2_state,
        }
    }

    /// Returns a mutable reference to the state for the given player.
    #[must_use]
    pub fn player_mut(&mut self, player: Player) -> &mut PlayerState {
        match player {
            Player::Player1 => &mut self.p1_state,
            Player::Player2 => &mut self.p2_state,
        }
    }

    /// Given the current player's hole selection, updates the game state.
    /// Panics if `hole >= HOLES_PER_SIDE` or the selected hole is empty.
    /// If debug assertions are enabled, panics if this state is a terminal state.
    pub fn make_move(&mut self, hole: usize) {
        debug_assert!(self.result().is_none()); // assert that this is not a terminal state

        let cur_player = self.cur_player;

        // take the stones out of the selected hole
        assert!(hole < HOLES_PER_SIDE, "invalid hole index: {hole}");
        let mut num_stones = mem::take(&mut self.player_mut(cur_player).holes[hole]) as usize;
        assert!(num_stones > 0, "selected an empty hole");

        // repeatedly place stones in successive spots
        let mut player = self.cur_player;
        let mut hole = Some(hole);

        while num_stones > 0 {
            // advance to the next hole, and add a stone to it
            match &mut hole {
                None => {
                    hole = Some(HOLES_PER_SIDE - 1);
                    player = player.other();
                    self.player_mut(player).holes[HOLES_PER_SIDE - 1] += 1;
                }
                Some(0) => {
                    if player != cur_player {
                        hole = Some(HOLES_PER_SIDE - 1);
                        player = player.other();
                        self.player_mut(player).holes[HOLES_PER_SIDE - 1] += 1;
                    } else {
                        hole = None;
                        self.player_mut(player).store += 1;
                    }
                }
                Some(hole) => {
                    *hole -= 1;
                    self.player_mut(player).holes[*hole] += 1;
                }
            }
            num_stones -= 1;
        }

        // handle conditions based on where the last stone was placed
        if player == cur_player {
            if let Some(hole) = hole {
                if self.player(cur_player).holes[hole] == 1 {
                    // the last stone landed in an empty hole on the current player's side;
                    // capture any stones in the opposite hole
                    let other_hole_idx = (HOLES_PER_SIDE - 1) - hole;
                    let captured_stones =
                        mem::take(&mut self.player_mut(cur_player.other()).holes[other_hole_idx]);
                    if captured_stones > 0 {
                        // additionally capture the 1 stone that landed in the empty hole
                        self.player_mut(cur_player).holes[hole] = 0;
                        let captured_stones = captured_stones + 1;

                        self.player_mut(cur_player).store += captured_stones;
                    }
                }
            } else {
                // the last stone landed in the current player's store;
                // flip the current player now so they get another turn
                self.cur_player = self.cur_player.other();
            }
        }

        // finally, toggle whose turn it is
        self.cur_player = self.cur_player.other();
    }

    /// Returns the final game result Some((P1 score) - (P2 score)), or None
    /// if the game is not yet over in this state.
    #[must_use]
    pub fn result(&self) -> Option<i8> {
        let p1_stones = self.p1_state.stones_in_holes();
        let p2_stones = self.p2_state.stones_in_holes();
        if p1_stones == 0 || p2_stones == 0 {
            let p1_score = self.p1_state.store + p1_stones;
            let p2_score = self.p2_state.store + p2_stones;
            return Some((p1_score as i8) - (p2_score as i8)); // the game is over with this score
        }
        None // the game isn't over yet
    }

    /// Returns an iterator over the valid moves that can be made from this
    /// state, in ascending order.
    pub fn valid_moves(&self) -> impl Iterator<Item = usize> + '_ {
        self.player(self.cur_player).non_empty_holes()
    }
}

/// Represents the state for a single player (their holes and store).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PlayerState {
    /// The player's holes. Index 0 is closest to this player's store.
    pub holes: [u8; HOLES_PER_SIDE],

    /// The player's store.
    pub store: u8,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            holes: [INITIAL_STONES_PER_HOLE; HOLES_PER_SIDE],
            store: 0,
        }
    }
}

impl PlayerState {
    /// Returns the total number of stones in the holes on this player's side.
    #[must_use]
    pub fn stones_in_holes(&self) -> u8 {
        self.holes.iter().sum()
    }

    /// Returns this player's score (assuming this state is at the end of a game).
    #[must_use]
    pub fn score(&self) -> u8 {
        self.store + self.stones_in_holes()
    }

    /// Returns an iterator over the indices of the non-empty holes on this
    /// player's side, in ascending order.
    pub fn non_empty_holes(&self) -> impl Iterator<Item = usize> + '_ {
        self.holes
            .iter()
            .enumerate()
            .filter(|(_i, h)| **h > 0)
            .map(|(i, _)| i)
    }
}
