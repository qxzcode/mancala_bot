use std::{
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use egui::{mutex::Mutex, Context};

use crate::{
    game_state::GameState,
    mcts::{MCTSContext, StateStats},
};

/// A message from the main thread to the worker thread.
enum Message {
    /// Stop the worker thread.
    Stop,

    /// Clear the explored node cache.
    ClearCache,

    /// Set the active game state to work on.
    SetActiveState(GameState),
}

/// Data representing the state of the worker thread's computation and results.
#[derive(Clone)]
pub struct WorkerData {
    pub game_state: GameState,
    pub stats: StateStats,
    pub node_cache_size: usize,
}

/// Manages the worker thread performing game computations and facilitates
/// inter-thread communicaiton.
pub struct Worker {
    /// The join handle for the worker thread.
    join_handle: Option<JoinHandle<()>>,

    /// Sender for sending control messages to the worker thread.
    message_sender: Sender<Message>,

    /// The latest data from the worker thread.
    cur_data: Arc<Mutex<Option<WorkerData>>>,
}

impl Worker {
    /// Spawns a new worker thread and returns a `Worker` manager for it.
    #[must_use]
    pub fn spawn(ui_context: &Context) -> Self {
        let cur_state_and_data = Arc::new(Mutex::new(None));
        let cur_state_and_data2 = cur_state_and_data.clone();
        let (sender, receiver) = mpsc::channel();
        let ui_context = ui_context.clone();

        let join_handle = thread::Builder::new()
            .name("worker".into())
            .spawn(move || {
                println!("Worker thread started");
                let update_delay = Duration::from_secs_f64(1.0 / 60.0); // delay between UI updates
                let mut mcts_context = MCTSContext::new();
                let mut active_game_state = None;

                let send_update = |mcts_context: &MCTSContext, game_state: &GameState| {
                    let new_state_and_data =
                        mcts_context.stats_for(game_state).map(|stats| WorkerData {
                            game_state: game_state.clone(),
                            stats: stats.clone(),
                            node_cache_size: mcts_context.cache_size(),
                        });
                    *cur_state_and_data2.lock() = new_state_and_data;
                    ui_context.request_repaint();
                };

                'main_loop: loop {
                    // handle any messages sent from the main thread
                    for message in receiver.try_iter() {
                        match message {
                            Message::Stop => break 'main_loop,
                            Message::ClearCache => mcts_context.clear_cache(),
                            Message::SetActiveState(game_state) => {
                                send_update(&mcts_context, &game_state);
                                active_game_state = Some(game_state);
                            }
                        }
                    }

                    match &active_game_state {
                        Some(game_state) if game_state.result().is_none() => {
                            // do some MCTS computation
                            mcts_context.ponder(game_state, update_delay);

                            // update the state data that the main thread can access
                            send_update(&mcts_context, game_state);
                        }
                        _ => thread::sleep(update_delay),
                    }
                }
            })
            .expect("failed to spawn worker thread");

        Self {
            join_handle: Some(join_handle),
            message_sender: sender,
            cur_data: cur_state_and_data,
        }
    }

    /// Sets the active game state that the worker should compute on.
    pub fn set_active_state(&self, game_state: GameState) {
        self.message_sender
            .send(Message::SetActiveState(game_state))
            .expect("failed to send to worker thread");
    }

    /// Clears the explored node cache.
    pub fn clear_cache(&self) {
        self.message_sender
            .send(Message::ClearCache)
            .expect("failed to send to worker thread");
    }

    /// Gets the current worker data
    pub fn get_data(&self) -> Option<WorkerData> {
        self.cur_data.lock().clone()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.message_sender.send(Message::Stop);
        self.join_handle
            .take()
            .unwrap()
            .join()
            .expect("worker thread should not panic");
    }
}
