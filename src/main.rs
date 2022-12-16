pub mod game_state;
pub mod mcts;

use rand::prelude::*;

use crate::game_state::{GameState, HOLES_PER_SIDE};

fn main() {
    println!("Hello, world!");
    println!("{}", std::mem::size_of::<GameState>());

    let random_move = || thread_rng().gen_range(0..HOLES_PER_SIDE);

    let mut game_state = GameState::default();
    loop {
        println!("{:?}", game_state);
        game_state.make_move(random_move()).unwrap();
    }
}
